mod bindings {
    use crate::LlmFetcher;
    wit_bindgen::generate!({
        with: {
            "wasi:clocks/monotonic-clock@0.2.2": ::wasi::clocks::monotonic_clock,
            "wasi:http/incoming-handler@0.2.2": generate,
            "wasi:http/outgoing-handler@0.2.2": ::wasi::http::outgoing_handler,
            "wasi:http/types@0.2.2": ::wasi::http::types,
            "wasi:io/error@0.2.2": ::wasi::io::error,
            "wasi:io/poll@0.2.2": ::wasi::io::poll,
            "wasi:io/streams@0.2.2": ::wasi::io::streams,
        }
    });
    export!(LlmFetcher);
}
use std::io::{Read as _, Write as _};

use bindings::exports::wasi::http::incoming_handler::Guest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use wasi::http::types::*;

struct LlmFetcher;

impl Guest for LlmFetcher {
    fn handle(_request: IncomingRequest, response_out: ResponseOutparam) {
        let req = wasi::http::outgoing_handler::OutgoingRequest::new(Fields::new());
        req.set_method(&Method::Post).unwrap(); // Set method to POST
        req.set_scheme(Some(&Scheme::Https)).unwrap();
        req.set_authority(Some("api.together.xyz")).unwrap(); // Authority without "https://"
        req.set_path_with_query(Some("/v1/chat/completions"))
            .unwrap();

        let mut headers = Fields::new();

        let content_type_value = "application/json".to_string().into_bytes();
        headers
            .set(&"Content-Type".to_string(), &[content_type_value])
            .expect("failed to set Content-Type header");

        let user_agent_value = "MyClient/1.0.0".to_string().into_bytes();
        headers
            .set(&"User-Agent".to_string(), &[user_agent_value])
            .expect("failed to set User-Agent header");

        let api_key =
            std::env::var("TOGETHER_API_KEY").unwrap_or_else(|_| "your_api_key".to_string());
        let bearer_token = format!("Bearer {}", api_key);

        let auth_value = bearer_token.into_bytes();
        headers
            .set(&"Authorization".to_string(), &[auth_value])
            .expect("failed to set Authorization header");

        let req = wasi::http::outgoing_handler::OutgoingRequest::new(headers);
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
        req.write().unwrap().write_all(&body_bytes).unwrap();



        match wasi::http::outgoing_handler::handle(req, None) {
            Ok(promise) => {
                promise.subscribe().block();
                match promise.get().expect("HTTP request failed") {
                    Ok(response) => {
                        let status = response.expect("failed http").status();
                        let mut outgoing_response = OutgoingResponse::new(Fields::new());
                        outgoing_response.set_status_code(status).unwrap();

                        let mut body_vec = Vec::new();
                        response.unwrap().into().unwrap().read_to_end(&mut body_vec).unwrap();

                        if status == 200 {
                            // Parse the response body
                           let completion_response: CreateChatCompletionResponseExt = serde_json::from_slice(&body_vec).unwrap_or_else(|e| {
                                eprintln!("Deserialization error: {:?}", e); // Log or otherwise handle error.
                                CreateChatCompletionResponseExt { id:"", object: "", created: 0, model: "", choices: vec![], usage: None } // Return a default/empty value
                           });


                           let joke = completion_response.choices.get(0).and_then(|choice| choice.message.content.clone()).unwrap_or_else(|| "No joke found.".to_string());
                           outgoing_response. write().unwrap().write_all(joke.as_bytes()).unwrap();

                        } else {
                             outgoing_response.write().unwrap().write_all(&body_vec).unwrap();
                        }

                        ResponseOutparam::set(response_out, Ok(outgoing_response));
                    }
                    Err(e) => {
                       // ... (Error handling remains unchanged)
                        let mut outgoing_response = OutgoingResponse::new(Fields::new());
                        outgoing_response.set_status_code(500).unwrap();
                        outgoing_response.write().unwrap().write_all(format!("HTTP request error: {:?}", e).as_bytes()).unwrap();
                        ResponseOutparam::set(response_out, Ok(outgoing_response));

                    }
                }
            }
            Err(e) => {
                //  ... (Error handling remains unchanged)

            }
        }
    }
}      

// Additional structs for parsing the response

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
