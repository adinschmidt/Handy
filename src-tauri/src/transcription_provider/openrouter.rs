use super::ProviderTranscript;
use crate::audio_toolkit::encode_wav_bytes;
use anyhow::{anyhow, Context, Result};
use reqwest::multipart::{Form, Part};
use serde::{Deserialize, Serialize};
use std::time::Duration;

const BASE_URL: &str = "https://openrouter.ai/api/v1";

#[derive(Deserialize, Serialize, specta::Type)]
pub struct OpenrouterModel {
    pub id: String,
    pub name: String,
}

#[derive(Deserialize)]
struct ModelCatalog {
    data: Vec<OpenrouterModel>,
}

#[derive(Deserialize)]
struct TranscriptionResponse {
    text: String,
}

fn require_key(api_key: &str) -> Result<&str> {
    let key = api_key.trim();
    if key.is_empty() {
        return Err(anyhow!("OpenRouter API key is required"));
    }
    Ok(key)
}

fn check_status(response: &reqwest::Response) -> Result<()> {
    let status = response.status();
    if !status.is_success() {
        let hint = match status.as_u16() {
            401 | 403 => " Check the API key in Cloud transcription settings.",
            402 => " Check your OpenRouter credit balance.",
            429 => " Try again after the rate limit resets.",
            _ => "",
        };
        // Response bodies can contain provider diagnostics or submitted content.
        return Err(anyhow!("OpenRouter returned HTTP {}.{}", status, hint));
    }
    Ok(())
}

pub async fn fetch_models(api_key: &str) -> Result<Vec<OpenrouterModel>> {
    fetch_models_at(BASE_URL, api_key).await
}

async fn fetch_models_at(base_url: &str, api_key: &str) -> Result<Vec<OpenrouterModel>> {
    let response = super::http_client()?
        .get(format!("{base_url}/models"))
        .query(&[("output_modalities", "transcription")])
        .bearer_auth(require_key(api_key)?)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .context("Could not fetch OpenRouter transcription models")?;
    check_status(&response)?;
    let mut catalog: ModelCatalog = response
        .json()
        .await
        .context("OpenRouter returned an invalid model catalog")?;
    catalog.data.retain(|model| !model.id.trim().is_empty());
    catalog
        .data
        .sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    Ok(catalog.data)
}

pub async fn transcribe(
    api_key: &str,
    model: &str,
    samples: &[f32],
    language: Option<&str>,
) -> Result<ProviderTranscript> {
    transcribe_at(BASE_URL, api_key, model, samples, language).await
}

async fn transcribe_at(
    base_url: &str,
    api_key: &str,
    model: &str,
    samples: &[f32],
    language: Option<&str>,
) -> Result<ProviderTranscript> {
    let api_key = require_key(api_key)?;
    if model.trim().is_empty() {
        return Err(anyhow!("Select an OpenRouter transcription model first"));
    }
    let wav = encode_wav_bytes(samples).context("Failed to encode recording as WAV")?;
    let file = Part::bytes(wav)
        .file_name("recording.wav")
        .mime_str("audio/wav")?;
    let mut form = Form::new()
        .part("file", file)
        .text("model", model.to_string());
    if let Some(language) = language {
        form = form.text("language", language.to_string());
    }
    let response = super::http_client()?
        .post(format!("{base_url}/audio/transcriptions"))
        .bearer_auth(api_key)
        .multipart(form)
        .send()
        .await
        .context("Could not reach OpenRouter")?;
    check_status(&response)?;
    let response: TranscriptionResponse = response
        .json()
        .await
        .context("OpenRouter returned an invalid transcription response")?;
    Ok(ProviderTranscript::plain(response.text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn discovers_transcription_models_with_saved_key() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/models"))
            .and(query_param("output_modalities", "transcription"))
            .and(header("authorization", "Bearer test-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"id": "vendor/new-stt", "name": "New speech model"}]
            })))
            .expect(1)
            .mount(&server)
            .await;
        let models = fetch_models_at(&server.uri(), "test-key").await.unwrap();
        assert_eq!(models[0].id, "vendor/new-stt");
        assert!(fetch_models_at(&server.uri(), " ").await.is_err());
    }

    #[tokio::test]
    async fn uploads_wav_with_selected_model_and_language() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/audio/transcriptions"))
            .and(header("authorization", "Bearer test-key"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"text": "Hello."})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let transcript = transcribe_at(
            &server.uri(),
            "test-key",
            "vendor/stt",
            &[0.0, 0.25],
            Some("en"),
        )
        .await
        .unwrap();
        assert_eq!(transcript.text, "Hello.");
        let requests = server.received_requests().await.unwrap();
        let body = String::from_utf8_lossy(&requests[0].body);
        for expected in [
            "name=\"file\"",
            "recording.wav",
            "RIFF",
            "WAVE",
            "name=\"model\"",
            "vendor/stt",
            "name=\"language\"",
            "\r\nen\r\n",
        ] {
            assert!(body.contains(expected), "Missing {expected}");
        }
    }

    #[tokio::test]
    async fn omits_auto_language_and_hides_error_bodies() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(401).set_body_string("secret diagnostic"))
            .mount(&server)
            .await;
        let error = transcribe_at(&server.uri(), "test-key", "vendor/stt", &[0.0], None)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("Check the API key"));
        assert!(!error.contains("secret diagnostic"));
        let requests = server.received_requests().await.unwrap();
        assert!(!String::from_utf8_lossy(&requests[0].body).contains("name=\"language\""));
        assert!(transcribe_at(&server.uri(), "", "vendor/stt", &[0.0], None)
            .await
            .is_err());
        assert!(transcribe_at(&server.uri(), "test-key", "", &[0.0], None)
            .await
            .is_err());
    }
}
