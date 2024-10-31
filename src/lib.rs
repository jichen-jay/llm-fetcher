mod bindings {
    use crate::LlmFetcher;

    wit_bindgen::generate!({
        with: {
            "wasi:clocks/monotonic-clock@0.2.2": ::wasi::clocks::monotonic_clock,
            "wasi:http/incoming-handler@0.2.2": generate,
            "wasi:http/outgoing-handler@0.2.2": ::wasi::http::outgoing_handler,
            "wasi:http/types@0.2.2": ::wasi::http::types,
            "wasi:io/error@0.2.2": ::wasi::io::error,
            "wasi:io/streams@0.2.2": ::wasi::io::streams,
            "wasi:io/poll@0.2.2": ::wasi::io::poll,
        }
    });
    export!(LlmFetcher);
}
use bindings::exports::wasi::http::incoming_handler::Guest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use wasi::http::outgoing_handler;
use wasi::http::types::*;
use wasi::io::streams::{InputStream, OutputStream, StreamError};

struct LlmFetcher;
impl Guest for LlmFetcher {
    fn handle(_request: IncomingRequest, response_out: ResponseOutparam) {
        let headers = Fields::new();

        let req = OutgoingRequest::new(headers.clone());

        req.set_method(&Method::Post).unwrap();
        req.set_scheme(Some(&Scheme::Https)).unwrap();
        req.set_authority(Some("api.together.xyz")).unwrap();
        req.set_path_with_query(Some("/v1/chat/completions"))
            .unwrap();

        // Set headers
        headers
            .set(
                &"Content-Type".to_string(),
                &["application/json".as_bytes().to_vec()],
            )
            .expect("failed to set Content-Type header");

        headers
            .set(
                &"User-Agent".to_string(),
                &["MyClient/1.0.0".as_bytes().to_vec()],
            )
            .expect("failed to set User-Agent header");

        let api_key =
            std::env::var("TOGETHER_API_KEY").unwrap_or_else(|_| "your_api_key".to_string());
        let bearer_token = format!("Bearer {}", api_key);
        headers
            .set(
                &"Authorization".to_string(),
                &[bearer_token.as_bytes().to_vec()],
            )
            .expect("failed to set Authorization header");

        let body = {
            let messages = json!([
                {"role": "system", "content": "You are a helpful assistant."},
                {"role": "user", "content": "tell me a joke from 1900s"}
            ]);

            json!({
                "model": "meta-llama/Meta-Llama-3.1-70B-Instruct-Turbo",
                "messages": messages,
                "max_tokens": 8192,
                "temperature": 0.3
            })
        };

        let body_bytes = serde_json::to_vec(&body).unwrap();

        // Write the body bytes to the request
        let outgoing_body = req.body().unwrap();
        let output_stream = outgoing_body.write().unwrap();
        write_all(&output_stream, &body_bytes).expect("Failed to write request body");

        // Finish the outgoing body
        OutgoingBody::finish(outgoing_body, None).expect("Failed to finish outgoing body");

        match outgoing_handler::handle(req, None) {
            Ok(promise) => {
                promise.subscribe().block();
                match promise.get().expect("HTTP request failed") {
                    Ok(response_result) => {
                        let response = response_result.expect("Failed to get response");
                        let status = response.status();

                        let outgoing_response = OutgoingResponse::new(Fields::new());
                        outgoing_response.set_status_code(status).unwrap();

                        // Read the body from the response
                        let incoming_body = response.consume().unwrap();
                        let input_stream = incoming_body.stream().unwrap();

                        let mut body_vec = Vec::new();
                        read_to_end(&input_stream, &mut body_vec)
                            .expect("Failed to read response body");

                        if status == 200 {
                            let response_body = outgoing_response.body().unwrap();
                            let output_stream = response_body.write().unwrap();

                            let completion_response: CreateChatCompletionResponseExt =
                                serde_json::from_slice(&body_vec).unwrap_or_else(|e| {
                                    eprintln!("Deserialization error: {:?}", e);
                                    CreateChatCompletionResponseExt {
                                        id: "".to_string(),
                                        object: "".to_string(),
                                        created: 0,
                                        model: "".to_string(),
                                        choices: vec![],
                                        usage: None,
                                    }
                                });

                            let joke = completion_response
                                .choices
                                .get(0)
                                .and_then(|choice| choice.message.content.clone())
                                .unwrap_or("No joke found.".to_string());

                            write_all(&output_stream, joke.as_bytes()).unwrap();

                            drop(output_stream);

                            OutgoingBody::finish(response_body, None)
                                .expect("failed to finish response body");
                        } else {
                            let response_body = outgoing_response.body().unwrap();
                            let output_stream = response_body.write().unwrap();
                            write_all(&output_stream, &body_vec).unwrap();
                            drop(output_stream);

                            OutgoingBody::finish(response_body, None)
                                .expect("failed to finish response body");
                        }

                        ResponseOutparam::set(response_out, Ok(outgoing_response));
                    }
                    Err(e) => {
                        let outgoing_response = OutgoingResponse::new(Fields::new());
                        outgoing_response.set_status_code(500).unwrap();
                        let response_body = outgoing_response.body().unwrap();
                        let output_stream = response_body.write().unwrap();
                        let error_message = format!("HTTP request error: {:?}", e);
                        write_all(&output_stream, error_message.as_bytes()).unwrap();
                        drop(output_stream);

                        OutgoingBody::finish(response_body, None)
                            .expect("failed to finish response body");

                        ResponseOutparam::set(response_out, Ok(outgoing_response));
                    }
                }
            }
            Err(e) => {
                // Handle error from `handle` function
                eprintln!("Error handling request: {:?}", e);
            }
        }
    }
}

// Implement the write_all function
fn write_all(stream: &OutputStream, data: &[u8]) -> Result<(), StreamError> {
    let mut offset = 0;
    while offset < data.len() {
        let chunk_size = std::cmp::min(4096, data.len() - offset);
        let chunk = &data[offset..offset + chunk_size];
        stream.blocking_write_and_flush(chunk)?;
        offset += chunk_size;
    }
    Ok(())
}

// Implement the read_to_end function
fn read_to_end(stream: &InputStream, buffer: &mut Vec<u8>) -> Result<(), StreamError> {
    loop {
        let bytes = stream.blocking_read(4096)?;
        if bytes.is_empty() {
            break; // End of stream
        }
        buffer.extend_from_slice(&bytes);
    }
    Ok(())
}

// Your struct definitions
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ChoiceExt {
    pub index: u32,
    pub message: MessageExt,
    pub finish_reason: Option<FinishReasonExt>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MessageExt {
    pub role: String,
    pub content: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CreateChatCompletionResponseExt {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChoiceExt>,
    pub usage: Option<UsageExt>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub enum FinishReasonExt {
    Stop,
    Length,
    ContentFilter,
    FunctionCall,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct UsageExt {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}
