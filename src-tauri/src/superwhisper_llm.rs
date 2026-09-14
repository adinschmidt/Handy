use crate::settings::{SuperwhisperCredentials, SUPERWHISPER_USER_AGENT};
use serde::Deserialize;
use serde_json::{json, Value};

const API: &str = "https://api.superwhisper.com";
const MAX_OUTPUT_TOKENS: u32 = 8192;
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
const INVALID_RESPONSE: &str = "Superwhisper returned an invalid language-model response.";

#[derive(Clone, Copy)]
enum Format {
    OpenAi,
    Gemini,
    Anthropic,
}

struct Model {
    id: &'static str,
    path: &'static str,
    format: Format,
}

const MODELS: [Model; 3] = [
    Model {
        id: "gemini-3.7-flash",
        path: "/gemini/v1/messages",
        format: Format::Gemini,
    },
    Model {
        id: "gpt-5.6-luna",
        path: "/v1/chat/completions",
        format: Format::OpenAi,
    },
    Model {
        id: "claude-sonnet-5",
        path: "/anthropic/v1/messages",
        format: Format::Anthropic,
    },
];

fn request(
    credentials: SuperwhisperCredentials<'_>,
    method: reqwest::Method,
    url: String,
) -> Result<reqwest::RequestBuilder, String> {
    let credentials = SuperwhisperCredentials::new(
        credentials.x_id,
        credentials.x_license,
        credentials.x_signature,
    )?;
    let client = crate::transcription_provider::http_client()
        .map_err(|_| "Could not create the Superwhisper client.".to_string())?;
    Ok(client
        .request(method, url)
        .header("User-Agent", SUPERWHISPER_USER_AGENT)
        .header("X-Platform", "macos")
        .header("X-ID", credentials.x_id)
        .header("X-License", credentials.x_license)
        .header("X-Signature", credentials.x_signature))
}

async fn read_response(request: reqwest::RequestBuilder) -> Result<String, String> {
    let mut response = request
        .send()
        .await
        .map_err(|_| "Could not reach Superwhisper language models.".to_string())?;
    if !response.status().is_success() {
        return Err(format!(
            "Superwhisper language model returned HTTP {}.",
            response.status().as_u16()
        ));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Superwhisper language-model response was interrupted.".to_string())?
    {
        if bytes.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return Err("Superwhisper language-model response exceeded the size limit.".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    String::from_utf8(bytes).map_err(|_| INVALID_RESPONSE.into())
}

#[derive(Deserialize)]
struct Catalog {
    models: Vec<CatalogModel>,
}

#[derive(Deserialize)]
struct CatalogModel {
    id: String,
    #[serde(default)]
    deprecated: bool,
}

pub async fn fetch_models(credentials: SuperwhisperCredentials<'_>) -> Result<Vec<String>, String> {
    let body = read_response(request(
        credentials,
        reqwest::Method::GET,
        format!("{API}/models/language/cloud"),
    )?)
    .await?;
    let catalog: Catalog = serde_json::from_str(&body).map_err(|_| INVALID_RESPONSE.to_string())?;
    Ok(MODELS
        .iter()
        .filter(|model| {
            catalog
                .models
                .iter()
                .any(|entry| entry.id == model.id && !entry.deprecated)
        })
        .map(|model| model.id.to_string())
        .collect())
}

pub async fn complete(
    credentials: SuperwhisperCredentials<'_>,
    model: &str,
    prompt: &str,
) -> Result<String, String> {
    complete_at(API, credentials, model, prompt).await
}

async fn complete_at(
    api: &str,
    credentials: SuperwhisperCredentials<'_>,
    model: &str,
    prompt: &str,
) -> Result<String, String> {
    let model = MODELS
        .iter()
        .find(|entry| entry.id == model)
        .ok_or("Unsupported Superwhisper language model.")?;
    // The Gemini proxy also accepts messages, rather than Google's contents envelope.
    let mut body = json!({"model": model.id, "messages": [{"role": "user", "content": prompt}], "stream": true});
    let token_field = match model.format {
        Format::OpenAi => "max_completion_tokens",
        Format::Gemini | Format::Anthropic => "max_tokens",
    };
    body[token_field] = json!(MAX_OUTPUT_TOKENS);
    let response = read_response(
        request(
            credentials,
            reqwest::Method::POST,
            format!("{api}{}", model.path),
        )?
        .json(&body),
    )
    .await?;
    parse_stream(model.format, &response)
}

#[derive(Default)]
struct Completion {
    text: String,
    finished: bool,
    ended: bool,
}

impl Completion {
    fn finish(&mut self, reason: &str, expected: &[&str]) -> Result<(), String> {
        if ["length", "max_tokens", "MAX_TOKENS"].contains(&reason) {
            return Err("Superwhisper reached the output token limit; the incomplete rewrite was discarded.".into());
        }
        if !expected.contains(&reason) {
            return Err("Superwhisper did not finish the rewrite successfully.".into());
        }
        self.finished = true;
        Ok(())
    }

    fn event(&mut self, format: Format, data: &str) -> Result<(), String> {
        if data == "[DONE]" {
            self.ended = true;
            return Ok(());
        }
        let value: Value = serde_json::from_str(data).map_err(|_| INVALID_RESPONSE.to_string())?;
        if value.get("error").is_some() || value["type"] == "error" {
            return Err("Superwhisper reported a language-model error.".into());
        }
        match format {
            Format::OpenAi => {
                if let Some(choices) = value["choices"].as_array() {
                    for choice in choices.iter().filter(|choice| choice["index"] == 0) {
                        if choice["delta"]["refusal"]
                            .as_str()
                            .is_some_and(|text| !text.is_empty())
                        {
                            return Err("Superwhisper declined the rewrite.".into());
                        }
                        if let Some(text) = choice["delta"]["content"].as_str() {
                            self.text.push_str(text);
                        }
                        if let Some(reason) = choice["finish_reason"].as_str() {
                            self.finish(reason, &["stop"])?;
                        }
                    }
                }
            }
            Format::Gemini => {
                if let Some(candidates) = value["candidates"].as_array() {
                    for candidate in candidates
                        .iter()
                        .filter(|candidate| candidate["index"] == 0)
                    {
                        if let Some(parts) = candidate["content"]["parts"].as_array() {
                            for part in parts.iter().filter(|part| part["thought"] != true) {
                                if let Some(text) = part["text"].as_str() {
                                    self.text.push_str(text);
                                }
                            }
                        }
                        if let Some(reason) = candidate["finishReason"].as_str() {
                            self.finish(reason, &["STOP"])?;
                            self.ended = true;
                        }
                    }
                }
            }
            Format::Anthropic => match value["type"].as_str() {
                Some("content_block_start") if value["content_block"]["type"] == "text" => {
                    if let Some(text) = value["content_block"]["text"].as_str() {
                        self.text.push_str(text);
                    }
                }
                Some("content_block_delta") if value["delta"]["type"] == "text_delta" => {
                    if let Some(text) = value["delta"]["text"].as_str() {
                        self.text.push_str(text);
                    }
                }
                Some("message_delta") => {
                    if let Some(reason) = value["delta"]["stop_reason"].as_str() {
                        self.finish(reason, &["end_turn", "stop_sequence"])?;
                    }
                }
                Some("message_stop") => self.ended = true,
                _ => {}
            },
        }
        Ok(())
    }
}

fn parse_stream(format: Format, body: &str) -> Result<String, String> {
    let mut completion = Completion::default();
    let mut data = Vec::new();
    // Parse complete SSE events, including CRLF and multiline data fields.
    for line in body.lines().chain(std::iter::once("")) {
        if line.is_empty() {
            if !data.is_empty() {
                completion.event(format, &data.join("\n"))?;
                data.clear();
            }
        } else if let Some(value) = line.strip_prefix("data:") {
            data.push(value.strip_prefix(' ').unwrap_or(value));
        }
    }
    if !completion.finished || !completion.ended {
        return Err("Superwhisper response ended before the rewrite was complete.".into());
    }
    let text = completion.text.trim();
    let text = if let Some(inner) = text.strip_prefix("<sw_response_content>") {
        inner
            .strip_suffix("</sw_response_content>")
            .ok_or(INVALID_RESPONSE)?
            .trim()
    } else {
        text
    };
    if text.is_empty() {
        return Err("Superwhisper returned an empty rewrite.".into());
    }
    Ok(text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const OPENAI: &str = "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"<sw_response_content>Hello.\"},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"</sw_response_content>\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
    const GEMINI: &str = "data: {\"candidates\":[{\"index\":0,\"content\":{\"parts\":[{\"text\":\"private reasoning\",\"thought\":true},{\"text\":\"Hello.\"}]}}]}\r\n\r\ndata: {\"candidates\":[{\"index\":0,\"finishReason\":\"STOP\"}]}\r\n\r\n";
    const ANTHROPIC: &str = "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello.\"}}\n\nevent: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n\n";

    #[tokio::test]
    async fn routes_each_model_with_saved_credentials_and_collects_only_final_text() {
        let server = MockServer::start().await;
        let id = "00000000-0000-4000-8000-000000000001";
        let signature = "a".repeat(64);
        for (model, response) in MODELS.iter().zip([GEMINI, OPENAI, ANTHROPIC]) {
            let mut expected = json!({"model":model.id,"messages":[{"role":"user","content":"Clean this."}],"stream":true});
            expected[match model.format {
                Format::OpenAi => "max_completion_tokens",
                _ => "max_tokens",
            }] = json!(8192);
            Mock::given(method("POST"))
                .and(path(model.path))
                .and(header("X-ID", id))
                .and(header("X-License", id))
                .and(header("X-Signature", signature.as_str()))
                .and(header("X-Platform", "macos"))
                .and(body_json(expected))
                .respond_with(
                    ResponseTemplate::new(200)
                        .insert_header("Content-Type", "text/event-stream")
                        .set_body_string(response),
                )
                .expect(1)
                .mount(&server)
                .await;
            let result = complete_at(
                &server.uri(),
                SuperwhisperCredentials::new(id, id, &signature).unwrap(),
                model.id,
                "Clean this.",
            )
            .await
            .unwrap();
            assert_eq!(result, "Hello.");
        }
        for request in server.received_requests().await.unwrap() {
            assert!(!request.headers.contains_key("Authorization"));
            assert!(!request.headers.contains_key("x-api-key"));
        }
    }

    #[test]
    fn discards_truncated_interrupted_and_failed_streams() {
        for (format, body) in [
            (Format::OpenAi, OPENAI.replace("\"stop\"", "\"length\"")),
            (Format::Gemini, GEMINI.replace("STOP", "MAX_TOKENS")),
            (
                Format::Anthropic,
                ANTHROPIC.replace("end_turn", "max_tokens"),
            ),
        ] {
            assert!(parse_stream(format, &body)
                .unwrap_err()
                .contains("token limit"));
        }
        assert!(parse_stream(Format::OpenAi, &OPENAI.replace("data: [DONE]\n\n", "")).is_err());
        assert!(parse_stream(
            Format::Anthropic,
            &ANTHROPIC.replace(
                "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
                ""
            )
        )
        .is_err());
        assert!(parse_stream(Format::Gemini, &GEMINI.replace("STOP", "SAFETY")).is_err());
        let error = parse_stream(
            Format::OpenAi,
            "data: {\"error\":{\"message\":\"private prompt\"}}\n\n",
        )
        .unwrap_err();
        assert!(!error.contains("private prompt"));
    }

    #[tokio::test]
    async fn http_errors_do_not_expose_private_bodies() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(401).set_body_string("private credential and prompt"),
            )
            .mount(&server)
            .await;
        let id = "00000000-0000-4000-8000-000000000001";
        let signature = "a".repeat(64);
        let error = complete_at(
            &server.uri(),
            SuperwhisperCredentials::new(id, id, &signature).unwrap(),
            "gpt-5.6-luna",
            "Clean this.",
        )
        .await
        .unwrap_err();
        assert!(error.contains("401"));
        assert!(!error.contains("private"));
    }
}
