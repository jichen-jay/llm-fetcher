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
use serde_json::Value;
use std::io::{Read as _, Write as _};
use wasi::http::types::*;

struct LlmFetcher;

impl Guest for LlmFetcher {
    fn handle(request: IncomingRequest, response_out: ResponseOutparam) {
        let mut user_input = String::new();
        if let Some(inp) = request.consume().ok() {
            let mut body = Vec::new();
            if let Some(mut stream) = inp.stream().ok() {
                stream.read_to_end(&mut body).unwrap();
            }
            let _ = IncomingBody::finish(inp);
            let body_str = String::from_utf8_lossy(&body);
            if let Ok(json) = serde_json::from_str::<Value>(&body_str) {
                if let Some(text) = json.get("text").and_then(Value::as_str) {
                    user_input = text.to_string();
                    println!("User input: {}", user_input);
                }
            } else {
                println!("Failed to parse JSON");
            }
        }

        let headers = Fields::new();

        let content_type_name = "Content-Type".to_string();
        let content_type_values = vec!["application/json".as_bytes().to_vec()];
        headers
            .set(&content_type_name, &content_type_values)
            .expect("Failed to set Content-Type header");

        let user_agent_name = "User-Agent".to_string();
        let user_agent_values = vec!["MyClient/1.0.0".as_bytes().to_vec()];
        headers
            .set(&user_agent_name, &user_agent_values)
            .expect("Failed to set User-Agent header");

        let authorization_name = "Authorization".to_string();
        let api_key = std::env::var("TOGETHER_API_KEY").unwrap();
        let bearer_token = format!("Bearer {}", api_key);
        let authorization_values = vec![bearer_token.into_bytes()];
        headers
            .set(&authorization_name, &authorization_values)
            .expect("Failed to set Authorization header");

        let req = OutgoingRequest::new(headers);
        req.set_method(&Method::Post).unwrap();
        req.set_scheme(Some(&Scheme::Https)).unwrap();
        req.set_authority(Some("api.together.xyz")).unwrap();
        req.set_path_with_query(Some("/v1/chat/completions"))
            .unwrap();

        let body = {
            let messages = json!([
                {"role": "system", "content": "You are a helpful assistant."},
                {"role": "user", "content": user_input}
            ]);

            println!("msg: {:?}", user_input.to_string());
            json!({
                "model": "meta-llama/Meta-Llama-3.1-70B-Instruct-Turbo",
                "messages": messages,
                "max_tokens": 8192,
                "temperature": 0.3
            })
        };

        let body_bytes = serde_json::to_vec(&body).unwrap();

        if let Some(outgoing_body) = req.body().ok() {
            if let Some(mut write_stream) = outgoing_body.write().ok() {
                write_stream.write_all(&body_bytes).unwrap();
                drop(write_stream);
                OutgoingBody::finish(outgoing_body, None).unwrap();
            }
        }

        let llm_response = match wasi::http::outgoing_handler::handle(req, None) {
            Ok(promise) => {
                promise.subscribe().block();
                let response = promise
                    .get()
                    .expect("Failed to get response promise")
                    .expect("Failed to get response")
                    .expect("HTTP request failed");
                if response.status() == 200 {
                    if let Some(response_body) = response.consume().ok() {
                        let mut body = Vec::new();
                        if let Some(mut stream) = response_body.stream().ok() {
                            stream.read_to_end(&mut body).unwrap();
                        }
                        let _ = IncomingBody::finish(response_body);

                        let output = match serde_json::from_slice::<CreateChatCompletionResponseExt>(
                            &body,
                        ) {
                            Ok(response) => response
                                .choices
                                .get(0)
                                .and_then(|choice| choice.message.content.clone())
                                .unwrap_or_else(|| "No content found.".to_string()),
                            Err(e) => {
                                format!("Failed to deserialize response: {}", e)
                            }
                        };

                        output
                    } else {
                        "Failed to consume response body".to_string()
                    }
                } else {
                    if let Some(response_body) = response.consume().ok() {
                        let mut body = Vec::new();
                        if let Some(mut stream) = response_body.stream().ok() {
                            stream.read_to_end(&mut body).unwrap();
                        }
                        let _ = IncomingBody::finish(response_body);
                        let error_message = String::from_utf8_lossy(&body).to_string();
                        format!(
                            "HTTP request failed with status code {}: {}",
                            response.status(),
                            error_message
                        )
                    } else {
                        format!("HTTP request failed with status code {}", response.status())
                    }
                }
            }
            Err(e) => format!("Error during HTTP request: {}", e),
        };

        let response = OutgoingResponse::new(Fields::new());
        response.set_status_code(200).unwrap();
        let response_body = response.body().unwrap();
        let mut write_stream = response_body.write().unwrap();

        write_stream.write_all(llm_response.as_bytes()).unwrap();
        drop(write_stream);

        OutgoingBody::finish(response_body, None).expect("failed to finish response body");

        ResponseOutparam::set(response_out, Ok(response));
    }
}

//may not need to use it for now
// #[derive(Debug, Deserialize, Serialize, Clone)]
// pub enum FinishReasonExt {
//     Eos,
//     Stop,
//     Length,
//     ContentFilter,
//     FunctionCall,
// }

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CreateChatCompletionResponseExt {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    #[serde(default)]
    pub prompt: Vec<String>, // Assuming prompt is optional or sometimes missing
    pub choices: Vec<ChoiceExt>,
    pub usage: Option<UsageExt>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ChoiceExt {
    pub index: u32,
    pub message: MessageExt,
    pub finish_reason: Option<String>,
    #[serde(default)]
    pub seed: Option<u64>, // Optional if it may or may not be present
    #[serde(default)]
    pub logprobs: Option<serde_json::Value>, // Use Value if structure is unknown
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MessageExt {
    pub role: String,
    pub content: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<serde_json::Value>>, // Optional and flexible
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct UsageExt {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}
