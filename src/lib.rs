use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::Read;
use wasmcloud_component::http::{
    self,  ErrorCode, IncomingBody, OutgoingBody, Request, Response, Server, StatusCode,
};
use wasmcloud_component::wasi::http::types::{Fields, Method, OutgoingRequest, Scheme};
use wasmcloud_component::wasi::io::streams::{self, InputStream, StreamError};

struct LlmFetcher;

impl Server for LlmFetcher {
    fn handle(request: Request<IncomingBody>) -> Result<Response<impl OutgoingBody>, ErrorCode> {
        let input_stream = InputStream {
            handle: request.body(),
        };

        // Read the entire body into a Vec<u8>
        let body = read_to_end(&input_stream).map_err(|_| ErrorCode::ConfigurationError)?;

        let body_str = String::from_utf8_lossy(&body);

        // Parse the JSON from the body
        let user_input = match serde_json::from_str::<Value>(&body_str) {
            Ok(json) => {
                if let Some(text) = json.get("text").and_then(Value::as_str) {
                    text.to_string()
                } else {
                    return Err(ErrorCode::ConfigurationError);
                }
            }
            Err(_) => return Err(ErrorCode::ConfigurationError),
        };
        let headers = {
            let headers = Fields::new();
            headers
                .set(
                    &("Content-Type").to_string(),
                    &[b"application/json".to_vec()],
                )
                .map_err(|_| ErrorCode::ConfigurationError)?;
            headers
                .set(&("User-Agent").to_string(), &[b"MyClient/1.0.0".to_vec()])
                .map_err(|_| ErrorCode::ConfigurationError)?;
            let api_key =
                std::env::var("TOGETHER_API_KEY").map_err(|_| ErrorCode::ConfigurationError)?;
            let bearer_token = format!("Bearer {}", api_key);
            headers
                .set(
                    &("Authorization").to_string(),
                    &[bearer_token.as_bytes().to_vec()],
                )
                .map_err(|_| ErrorCode::ConfigurationError)?;
            headers
        };

        let req = {
            let request_builder = OutgoingRequest::new(headers);
            request_builder
                .set_method(&Method::Post)
                .map_err(|_| ErrorCode::ConfigurationError)?;
            request_builder
                .set_scheme(Some(&Scheme::Https))
                .map_err(|_| ErrorCode::ConfigurationError)?;
            request_builder
                .set_authority(Some("api.together.xyz"))
                .map_err(|_| ErrorCode::ConfigurationError)?;
            request_builder
                .set_path_with_query(Some("/v1/chat/completions"))
                .map_err(|_| ErrorCode::ConfigurationError)?;

            let body = {
                let messages = json!([
                    {"role": "system", "content": "You are a helpful assistant."},
                    {"role": "user", "content": user_input}
                ]);
                json!({
                    "model": "meta-llama/Meta-Llama-3.1-70B-Instruct-Turbo",
                    "messages": messages,
                    "max_tokens": 8192,
                    "temperature": 0.3
                })
            };

            let body_bytes =
                serde_json::to_vec(&body).map_err(|_| ErrorCode::ConfigurationError)?;

            if let Some(outgoing_body) = request_builder.body().ok() {
                if let Some(write_stream) = outgoing_body.write().ok() {
                    write_stream
                        .write(&body_bytes)
                        .map_err(|_| ErrorCode::ConfigurationError)?;
                }
            }

            request_builder
        };

        // Send the request and get the response
        let llm_response = match wasi::http::outgoing_handler::handle(req, None) {
            Ok(promise) => {
                promise.subscribe().block();
                let response = promise
                    .get()
                    .expect("Failed to get response")
                    .map_err(|_| ErrorCode::ConfigurationError)?
                    .map_err(|_| ErrorCode::ConfigurationError)?;
                if response.status() == 200 {
                    if let Some(response_body) = response.consume().ok() {
                        let mut body = Vec::new();
                        if let Some(mut stream) = response_body.stream().ok() {
                            stream
                                .read_to_end(&mut body)
                                .map_err(|_| ErrorCode::ConfigurationError)?;
                        }

                        let output = match serde_json::from_slice::<CreateChatCompletionResponseExt>(
                            &body,
                        ) {
                            Ok(response) => response
                                .choices
                                .get(0)
                                .and_then(|choice| choice.message.content.clone())
                                .unwrap_or_else(|| "No content found.".to_string()),
                            Err(_) => "Failed to deserialize response".to_string(),
                        };

                        output
                    } else {
                        "Failed to consume response body".to_string()
                    }
                } else {
                    format!("HTTP request failed with status code {}", response.status())
                }
            }
            Err(_) => "Error during HTTP request".to_string(),
        };

        // Build and return the response
        let response_body = llm_response.into_bytes();
        let builder = Response::builder()
            .header("Content-Type", "text/plain")
            .status(StatusCode::OK);

        match builder.body(response_body) {
            Ok(res) => Ok(res),
            Err(_) => Err(ErrorCode::ConfigurationError),
        }
    }
}

fn read_to_end(stream: &InputStream) -> Result<Vec<u8>, StreamError> {
    let mut data = Vec::new();
    loop {
        let chunk = InputStream::read(stream, 8192)?; // Use `Streams::blocking_read` if preferred
        if chunk.is_empty() {
            break;
        }
        data.extend(chunk);
    }
    Ok(data)
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

http::export!(LlmFetcher);
