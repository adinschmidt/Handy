use crate::audio_toolkit::encode_wav_bytes;
use crate::settings::SuperwhisperCredentials;
use anyhow::{anyhow, Context, Result};
use reqwest::multipart::{Form, Part};
use serde::Deserialize;

const API_BASE_URL: &str = "https://api.superwhisper.com";
const TRANSCRIPT_PATH: &str = "elevenlabs/v1/transcribe";
const USER_AGENT: &str =
    "superwhisper/2.16.6 (com.superduper.superwhisper; build:2.16.6; macOS 26.5.2) Alamofire/5.8.0";
const ACCEPT_LANGUAGE: &str = "en-CA,en-US;q=0.9,en;q=0.8";

pub(super) fn normalize_language(language: &str) -> Option<String> {
    let language = language.trim().to_ascii_lowercase();
    let base = language.split(['-', '_']).next().unwrap_or_default();
    (!base.is_empty() && base != "auto").then(|| base.to_string())
}

#[derive(Deserialize)]
struct Response {
    text: String,
    #[serde(default)]
    words: Vec<Word>,
}

#[derive(Deserialize)]
struct Word {
    text: String,
    #[serde(default, rename = "type")]
    word_type: String,
}

pub async fn transcribe(
    credentials: SuperwhisperCredentials<'_>,
    samples: &[f32],
    language: Option<&str>,
    keyterms: &[String],
    tag_audio_events: Option<bool>,
) -> Result<super::ProviderTranscript> {
    transcribe_at(
        API_BASE_URL,
        credentials,
        samples,
        language,
        keyterms,
        tag_audio_events,
    )
    .await
}

async fn transcribe_at(
    base_url: &str,
    credentials: SuperwhisperCredentials<'_>,
    samples: &[f32],
    language: Option<&str>,
    keyterms: &[String],
    tag_audio_events: Option<bool>,
) -> Result<super::ProviderTranscript> {
    // Validate here too so malformed imported settings never reach the network.
    let credentials = SuperwhisperCredentials::new(
        credentials.x_id,
        credentials.x_license,
        credentials.x_signature,
    )
    .map_err(|error| anyhow!(error))?;
    let wav = encode_wav_bytes(samples).context("Failed to encode recording as WAV")?;
    let mut form = Form::new().part(
        "file",
        Part::bytes(wav)
            .file_name("recording.wav")
            .mime_str("audio/wav")?,
    );
    if let Some(enabled) = tag_audio_events {
        form = form.text("tag_audio_events", enabled.to_string());
    }
    if let Some(language) = language.and_then(normalize_language) {
        form = form.text("language_code", language);
    }
    // Bound vocabulary hints without truncating a term into a different word.
    for term in keyterms
        .iter()
        .map(|term| term.trim())
        .filter(|term| !term.is_empty() && term.chars().count() <= 100)
        .take(100)
    {
        form = form.text("keyterms[]", term.to_string());
    }
    let endpoint = format!("{}/{}", base_url.trim_end_matches('/'), TRANSCRIPT_PATH);
    let response = super::http_client()?
        .post(endpoint)
        .header("Accept", "*/*")
        .header("Accept-Language", ACCEPT_LANGUAGE)
        .header("User-Agent", USER_AGENT)
        .header("X-ID", credentials.x_id)
        .header("X-License", credentials.x_license)
        .header("X-Signature", credentials.x_signature)
        // This is the captured credential context, independent of Handy's OS.
        .header("X-Platform", "macos")
        .multipart(form)
        .send()
        .await
        .map_err(|error| {
            anyhow!(
                "Could not reach Superwhisper transcription: {}.",
                if error.is_timeout() {
                    "timeout"
                } else if error.is_connect() {
                    "connection failed"
                } else {
                    "request failed"
                }
            )
        })?;
    let status = response.status();
    if !status.is_success() {
        let hint = match status.as_u16() {
            401 | 403 => {
                " Refresh all three credentials from your authorized Superwhisper installation."
            }
            429 => " Rate or concurrency limit reached. Try again later.",
            _ => "",
        };
        return Err(anyhow!(
            "Superwhisper transcription returned HTTP {}.{}",
            status.as_u16(),
            hint
        ));
    }
    let response: Response = response
        .json()
        .await
        .map_err(|_| anyhow!("Superwhisper returned an invalid transcription response."))?;
    let mut transcript = super::ProviderTranscript::plain(response.text);
    for word in response
        .words
        .into_iter()
        .filter(|word| word.word_type == "audio_event" && !word.text.is_empty())
    {
        transcript.protect(word.text);
    }
    Ok(transcript)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::AppSettings;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    const ID: &str = "00000000-0000-4000-8000-000000000001";
    const LICENSE: &str = "00000000-0000-4000-8000-000000000002";
    const SIGNATURE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    fn credentials() -> SuperwhisperCredentials<'static> {
        SuperwhisperCredentials::new(ID, LICENSE, SIGNATURE).unwrap()
    }

    /// Sends one request with an explicitly supplied disposable WAV and credentials.
    #[tokio::test]
    #[ignore = "requires explicit authorization, SW_PROBE_WAV, and SW_X_* credentials"]
    async fn live_wav_compatibility_probe() {
        let x_id = std::env::var("SW_X_ID").expect("SW_X_ID is required");
        let x_license = std::env::var("SW_X_LICENSE").expect("SW_X_LICENSE is required");
        let x_signature = std::env::var("SW_X_SIGNATURE").expect("SW_X_SIGNATURE is required");
        let wav_path = std::env::var("SW_PROBE_WAV").expect("SW_PROBE_WAV is required");
        let expected = std::env::var("SW_PROBE_EXPECTED").expect("SW_PROBE_EXPECTED is required");
        assert!(!expected.trim().is_empty());
        let mut reader = hound::WavReader::open(wav_path).expect("Could not open disposable WAV");
        let spec = reader.spec();
        assert_eq!(
            (spec.channels, spec.sample_rate, spec.bits_per_sample),
            (1, 16000, 16)
        );
        assert_eq!(spec.sample_format, hound::SampleFormat::Int);
        let samples = reader
            .samples::<i16>()
            .map(|sample| sample.map(|value| value as f32 / i16::MAX as f32))
            .collect::<Result<Vec<_>, _>>()
            .expect("Invalid WAV samples");
        let mut settings = AppSettings {
            selected_transcription_provider:
                crate::settings::TranscriptionProvider::SuperwhisperScribe,
            selected_language: "en".into(),
            ..AppSettings::default()
        };
        settings
            .set_superwhisper_credentials(&x_id, &x_license, &x_signature)
            .unwrap();
        let transcript = super::super::transcribe_cloud(
            settings.selected_transcription_provider,
            &settings,
            &samples,
        )
        .await
        .unwrap()
        .finish(&settings);
        let normalized = |text: &str| {
            text.chars()
                .filter(|character| character.is_alphanumeric())
                .flat_map(char::to_lowercase)
                .collect::<String>()
        };
        assert!(
            normalized(&transcript).contains(&normalized(&expected)),
            "Transcript did not match the disposable phrase"
        );
    }

    #[tokio::test]
    async fn sends_captured_protocol_with_wav_and_preserves_events() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/elevenlabs/v1/transcribe"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "text": "hello (applause)", "language_code": "eng", "audio_duration_secs": 1,
                "words": [{"text": "hello"}, {"text": "(applause)", "type": "audio_event"}]
            })))
            .expect(1)
            .mount(&server)
            .await;
        let transcript = transcribe_at(
            &server.uri(),
            credentials(),
            &[0.0, 0.25],
            Some("en-CA"),
            &[" one ".into(), " ".into(), "two".into()],
            None,
        )
        .await
        .unwrap();
        assert_eq!(
            transcript.finish(&AppSettings::default()),
            "hello (applause)"
        );
        let requests = server.received_requests().await.unwrap();
        let request = &requests[0];
        for (key, value) in [
            ("Accept", "*/*"),
            ("Accept-Language", ACCEPT_LANGUAGE),
            ("User-Agent", USER_AGENT),
            ("X-ID", ID),
            ("X-License", LICENSE),
            ("X-Signature", SIGNATURE),
            ("X-Platform", "macos"),
        ] {
            assert_eq!(request.headers.get(key).unwrap().to_str().unwrap(), value);
        }
        let body = String::from_utf8_lossy(&request.body);
        assert!(body.contains("name=\"file\"; filename=\"recording.wav\""));
        assert!(body.contains("audio/wav"));
        assert!(body.contains("RIFF"));
        assert!(body.contains("name=\"language_code\"\r\n\r\nen\r\n"));
        assert_eq!(body.matches("name=\"keyterms[]\"").count(), 2);
        assert!(body.contains("\r\n\r\none\r\n"));
        assert!(body.contains("\r\n\r\ntwo\r\n"));
        for absent in ["diarize", "tag_audio_events", "model_id", "xi-api-key"] {
            assert!(!body.contains(absent));
        }
    }

    #[tokio::test]
    async fn sends_audio_event_preference_only_when_explicitly_selected() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"text":"hello"})),
            )
            .expect(3)
            .mount(&server)
            .await;
        for preference in [None, Some(true), Some(false)] {
            transcribe_at(&server.uri(), credentials(), &[0.0], None, &[], preference)
                .await
                .unwrap();
        }
        let requests = server.received_requests().await.unwrap();
        assert!(!String::from_utf8_lossy(&requests[0].body).contains("tag_audio_events"));
        for (request, value) in requests[1..].iter().zip(["true", "false"]) {
            let body = String::from_utf8_lossy(&request.body);
            assert!(body.contains(&format!("name=\"tag_audio_events\"\r\n\r\n{value}\r\n")));
        }
    }

    #[tokio::test]
    async fn auto_omits_language_and_optional_metadata_is_not_required() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"text": "hello"})),
            )
            .expect(1)
            .mount(&server)
            .await;
        assert_eq!(
            transcribe_at(
                &server.uri(),
                credentials(),
                &[0.0],
                Some("auto"),
                &[],
                None
            )
            .await
            .unwrap()
            .finish(&AppSettings::default()),
            "hello"
        );
        let requests = server.received_requests().await.unwrap();
        let body = String::from_utf8_lossy(&requests[0].body);
        assert!(!body.contains("language_code"));
        assert!(!body.contains("keyterms[]"));
    }

    #[tokio::test]
    async fn rejects_malformed_credentials_before_network_io() {
        let server = MockServer::start().await;
        for value in ["", "bad", "\r\nsecret"] {
            let invalid = SuperwhisperCredentials {
                x_id: value,
                ..credentials()
            };
            assert!(
                transcribe_at(&server.uri(), invalid, &[0.0], None, &[], None)
                    .await
                    .is_err()
            );
        }
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn errors_never_echo_response_bodies_or_credentials_and_do_not_retry() {
        for status in [401, 403, 429, 500, 200] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .respond_with(
                    ResponseTemplate::new(status)
                        .set_body_string(format!("{ID} {LICENSE} {SIGNATURE} private-transcript")),
                )
                .expect(1)
                .mount(&server)
                .await;
            let error = format!(
                "{:#}",
                transcribe_at(&server.uri(), credentials(), &[0.0], None, &[], None)
                    .await
                    .unwrap_err()
            );
            for secret in [ID, LICENSE, SIGNATURE, "private-transcript"] {
                assert!(!error.contains(secret));
            }
            if status != 200 {
                assert!(error.contains(&format!("HTTP {status}")));
            }
        }
    }
}
