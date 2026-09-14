use crate::transcription_provider::ProviderTranscript;
use anyhow::{anyhow, Context, Result};
use reqwest::multipart::Form;
use reqwest::StatusCode;
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
    api_key: Option<&str>,
    samples: &[f32],
    language: Option<&str>,
) -> Result<ProviderTranscript> {
    let file = super::opus_audio_part(samples).await?;
    let mut form = Form::new()
        .part("file", file)
        .text("model", "whisper-1")
        .text("response_format", "json");
    if let Some(language) = language {
        form = form.text("language", language.to_string());
    }

    let endpoint = format!("{}/{}", base_url.trim_end_matches('/'), TRANSCRIPT_PATH);
    let mut request = super::http_client()?.post(&endpoint).multipart(form);
    if let Some(api_key) = api_key {
        request = request.bearer_auth(api_key);
    }
    let response = request.send().await.map_err(|err| {
        anyhow!(
            "Could not reach Codex ASR at {}: {}. Confirm the server is running.",
            base_url,
            err
        )
    })?;

    let status = response.status();
    if !status.is_success() {
        // The server may or may not require a key, so point at the setting
        // rather than at a specific way of starting it.
        let hint = if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            " Check the API key in Cloud transcription settings."
        } else {
            ""
        };
        return Err(anyhow!("Codex ASR returned HTTP {}.{}", status, hint));
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

        let transcript = transcribe(&server.uri(), None, &[0.0, 0.25, -0.25], Some("en"))
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
        assert!(body.contains("filename=\"recording.ogg\""));
        assert!(body.contains("OggS"));
        assert!(body.contains("OpusHead"));
        assert!(body.contains("audio/ogg"));
        assert!(body.contains("name=\"model\""));
        assert!(body.contains("whisper-1"));
        assert!(body.contains("name=\"response_format\""));
        assert!(body.contains("json"));
        assert!(body.contains("name=\"language\""));
        assert!(body.contains("en"));
    }

    #[tokio::test]
    async fn sends_bearer_auth_when_a_key_is_configured() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"text": "hello"})),
            )
            .mount(&server)
            .await;

        transcribe(&server.uri(), Some("test-secret-key"), &[0.0], None)
            .await
            .unwrap();

        let requests = server.received_requests().await.unwrap();
        assert_eq!(
            requests[0]
                .headers
                .get("authorization")
                .unwrap()
                .to_str()
                .unwrap(),
            "Bearer test-secret-key"
        );
    }

    /// A server reachable under a base path (behind a reverse proxy, say) must
    /// keep that prefix when the transcription path is appended.
    #[tokio::test]
    async fn appends_the_transcript_path_to_a_base_path() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/asr/v1/audio/transcriptions"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"text": "hello"})),
            )
            .mount(&server)
            .await;

        let transcript = transcribe(&format!("{}/asr", server.uri()), None, &[0.0], None)
            .await
            .unwrap();
        assert_eq!(transcript.finish(&AppSettings::default()), "hello");
    }

    #[tokio::test]
    async fn omits_automatic_language_and_reports_http_errors() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(401).set_body_string("secret server detail"))
            .mount(&server)
            .await;

        let error = transcribe(&server.uri(), None, &[0.0], None)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("HTTP 401"));
        assert!(error.contains("Check the API key"));
        assert!(!error.contains("secret server detail"));

        let requests = server.received_requests().await.unwrap();
        let body = String::from_utf8_lossy(&requests[0].body);
        assert!(!body.contains("name=\"language\""));
    }
}
