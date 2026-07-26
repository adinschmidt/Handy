use crate::audio_toolkit::encode_wav_bytes;
use crate::transcription_provider::ProviderTranscript;
use anyhow::{anyhow, Context, Result};
use reqwest::multipart::{Form, Part};
use serde::Deserialize;

const TRANSCRIPT_PATH: &str = "v1/audio/transcriptions";

/// Reduce Handy's language selection to the ISO-639-1 code the OpenAI
/// transcription API expects. `auto` means "let the server decide".
pub(super) fn normalize_language(language: &str) -> Option<String> {
    if language == "auto" {
        return None;
    }
    if language == "zh" || language.starts_with("zh-") {
        return Some("zh".to_string());
    }
    Some(language.split('-').next().unwrap_or(language).to_string())
}

#[derive(Deserialize)]
struct CodexTranscriptionResponse {
    text: String,
}

pub async fn transcribe(
    base_url: &str,
    samples: &[f32],
    language: Option<&str>,
) -> Result<ProviderTranscript> {
    let wav = encode_wav_bytes(samples).context("Failed to encode recording as WAV")?;
    let file = Part::bytes(wav)
        .file_name("recording.wav")
        .mime_str("audio/wav")?;
    let mut form = Form::new()
        .part("file", file)
        .text("model", "whisper-1")
        .text("response_format", "json");
    if let Some(language) = language {
        form = form.text("language", language.to_string());
    }

    let endpoint = format!("{}/{}", base_url.trim_end_matches('/'), TRANSCRIPT_PATH);
    let response = super::http_client()?
        .post(&endpoint)
        .multipart(form)
        .send()
        .await
        .map_err(|err| {
            anyhow!(
                "Could not reach Codex ASR at {}: {}. Start codex-asr with --no-api-key.",
                base_url,
                err
            )
        })?;

    let status = response.status();
    if !status.is_success() {
        return Err(anyhow!(
            "Codex ASR returned HTTP {}. Confirm the server is running with --no-api-key.",
            status
        ));
    }

    let response: CodexTranscriptionResponse = response
        .json()
        .await
        .context("Codex ASR returned an invalid JSON response")?;
    Ok(ProviderTranscript::plain(response.text))
}

#[cfg(test)]
mod tests {
    use super::{normalize_language, transcribe};
    use crate::settings::AppSettings;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn normalizes_language_codes_to_iso_639_1() {
        assert_eq!(normalize_language("auto"), None);
        assert_eq!(normalize_language("zh-Hant"), Some("zh".to_string()));
        assert_eq!(normalize_language("en-US"), Some("en".to_string()));
        assert_eq!(normalize_language("yue"), Some("yue".to_string()));
    }

    #[tokio::test]
    async fn sends_openai_compatible_multipart_without_authorization() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "text": "hello from codex"
            })))
            .mount(&server)
            .await;

        let transcript = transcribe(&server.uri(), &[0.0, 0.25, -0.25], Some("en"))
            .await
            .unwrap();
        assert_eq!(
            transcript.finish(&AppSettings::default()),
            "hello from codex"
        );

        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        let request = &requests[0];
        assert!(!request.headers.contains_key("authorization"));
        let body = String::from_utf8_lossy(&request.body);
        assert!(body.contains("name=\"file\""));
        assert!(body.contains("filename=\"recording.wav\""));
        assert!(body.contains("RIFF"));
        assert!(body.contains("WAVE"));
        assert!(body.contains("name=\"model\""));
        assert!(body.contains("whisper-1"));
        assert!(body.contains("name=\"response_format\""));
        assert!(body.contains("json"));
        assert!(body.contains("name=\"language\""));
        assert!(body.contains("en"));
    }

    #[tokio::test]
    async fn omits_automatic_language_and_reports_http_errors() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(
                ResponseTemplate::new(401).set_body_string("authentication disabled required"),
            )
            .mount(&server)
            .await;

        let error = transcribe(&server.uri(), &[0.0], None)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("HTTP 401"));
        assert!(!error.contains("authentication disabled required"));

        let requests = server.received_requests().await.unwrap();
        let body = String::from_utf8_lossy(&requests[0].body);
        assert!(!body.contains("name=\"language\""));
    }
}
