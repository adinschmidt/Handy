use crate::audio_toolkit::encode_wav_bytes;
use crate::transcription_provider::ProviderTranscript;
use anyhow::{anyhow, Context, Result};
use reqwest::multipart::{Form, Part};
use serde::Deserialize;

const API_BASE_URL: &str = "https://api.elevenlabs.io";
const TRANSCRIPT_PATH: &str = "v1/speech-to-text";
const MODEL_ID: &str = "scribe_v2";

pub(super) fn normalize_language(language: &str) -> Option<String> {
    let base = language.split('-').next().unwrap_or(language);
    let code = match base {
        "auto" => return None,
        "af" => "afr",
        "am" => "amh",
        "ar" => "ara",
        "as" => "asm",
        "az" => "aze",
        "be" => "bel",
        "bg" => "bul",
        "bn" => "ben",
        "bs" => "bos",
        "ca" => "cat",
        "cs" => "ces",
        "cy" => "cym",
        "da" => "dan",
        "de" => "deu",
        "el" => "ell",
        "en" => "eng",
        "es" => "spa",
        "et" => "est",
        "fa" => "fas",
        "fi" => "fin",
        "fr" => "fra",
        "gl" => "glg",
        "gu" => "guj",
        "ha" => "hau",
        "he" => "heb",
        "hi" => "hin",
        "hr" => "hrv",
        "hu" => "hun",
        "hy" => "hye",
        "id" => "ind",
        "is" => "isl",
        "it" => "ita",
        "ja" => "jpn",
        "jw" => "jav",
        "ka" => "kat",
        "kk" => "kaz",
        "km" => "khm",
        "kn" => "kan",
        "ko" => "kor",
        "lb" => "ltz",
        "ln" => "lin",
        "lo" => "lao",
        "lt" => "lit",
        "lv" => "lav",
        "mi" => "mri",
        "mk" => "mkd",
        "ml" => "mal",
        "mn" => "mon",
        "mr" => "mar",
        "ms" => "msa",
        "mt" => "mlt",
        "my" => "mya",
        "ne" => "nep",
        "nl" => "nld",
        "no" => "nor",
        "oc" => "oci",
        "pa" => "pan",
        "pl" => "pol",
        "ps" => "pus",
        "pt" => "por",
        "ro" => "ron",
        "ru" => "rus",
        "sd" => "snd",
        "sk" => "slk",
        "sl" => "slv",
        "sn" => "sna",
        "so" => "som",
        "sr" => "srp",
        "sv" => "swe",
        "sw" => "swa",
        "ta" => "tam",
        "te" => "tel",
        "tg" => "tgk",
        "th" => "tha",
        "tl" => "fil",
        "tr" => "tur",
        "uk" => "ukr",
        "ur" => "urd",
        "uz" => "uzb",
        "vi" => "vie",
        "yo" => "yor",
        "yue" => "yue",
        "zh" => "zho",
        _ => return None,
    };
    Some(code.to_string())
}

#[derive(Deserialize)]
struct ElevenLabsTranscriptionResponse {
    text: String,
    #[serde(default)]
    words: Vec<ElevenLabsWord>,
}

#[derive(Deserialize)]
struct ElevenLabsWord {
    text: String,
    #[serde(rename = "type")]
    word_type: String,
}

/// Scribe reports detected non-speech sounds as `audio_event` words that are
/// also inlined into `text`. Shield them so text cleanup, which is tuned for
/// dictated speech, doesn't rewrite or strip them.
fn to_transcript(response: ElevenLabsTranscriptionResponse) -> ProviderTranscript {
    let mut transcript = ProviderTranscript::plain(response.text);
    for event in response
        .words
        .into_iter()
        .filter(|word| word.word_type == "audio_event")
        .map(|word| word.text)
    {
        transcript.protect(event);
    }
    transcript
}

pub async fn transcribe(
    api_key: &str,
    samples: &[f32],
    language: Option<&str>,
) -> Result<ProviderTranscript> {
    transcribe_at(API_BASE_URL, api_key, samples, language).await
}

async fn transcribe_at(
    base_url: &str,
    api_key: &str,
    samples: &[f32],
    language: Option<&str>,
) -> Result<ProviderTranscript> {
    if api_key.trim().is_empty() {
        return Err(anyhow!(
            "ElevenLabs API key is required. Add it in Cloud transcription settings."
        ));
    }

    let wav = encode_wav_bytes(samples).context("Failed to encode recording as WAV")?;
    let file = Part::bytes(wav)
        .file_name("recording.wav")
        .mime_str("audio/wav")?;
    let mut form = Form::new()
        .part("file", file)
        .text("model_id", MODEL_ID)
        .text("tag_audio_events", "true");
    if let Some(language) = language {
        form = form.text("language_code", language.to_string());
    }

    let endpoint = format!("{}/{}", base_url.trim_end_matches('/'), TRANSCRIPT_PATH);
    let response = super::http_client()?
        .post(endpoint)
        .header("xi-api-key", api_key.trim())
        .multipart(form)
        .send()
        .await
        .map_err(|err| anyhow!("Could not reach ElevenLabs speech-to-text: {}", err))?;

    let status = response.status();
    if !status.is_success() {
        return Err(anyhow!(
            "ElevenLabs speech-to-text returned HTTP {}",
            status
        ));
    }

    let response: ElevenLabsTranscriptionResponse = response
        .json()
        .await
        .context("ElevenLabs returned an invalid speech-to-text response")?;
    Ok(to_transcript(response))
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_language, to_transcript, transcribe_at, ElevenLabsTranscriptionResponse,
        ElevenLabsWord,
    };
    use crate::settings::AppSettings;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Only `audio_event` words are shielded; ordinary words and spacing stay
    /// subject to normal text cleanup.
    #[test]
    fn protects_audio_events_but_not_spoken_words() {
        let settings = AppSettings {
            app_language: "en".to_string(),
            custom_words: vec!["Handy".to_string()],
            ..AppSettings::default()
        };
        let response = ElevenLabsTranscriptionResponse {
            text: "handy um (applause)".to_string(),
            words: vec![
                ElevenLabsWord {
                    text: "handy".to_string(),
                    word_type: "word".to_string(),
                },
                ElevenLabsWord {
                    text: " ".to_string(),
                    word_type: "spacing".to_string(),
                },
                ElevenLabsWord {
                    text: "um".to_string(),
                    word_type: "word".to_string(),
                },
                ElevenLabsWord {
                    text: " ".to_string(),
                    word_type: "spacing".to_string(),
                },
                ElevenLabsWord {
                    text: "(applause)".to_string(),
                    word_type: "audio_event".to_string(),
                },
            ],
        };

        assert_eq!(
            to_transcript(response).finish(&settings),
            "Handy (applause)"
        );
    }

    #[test]
    fn maps_only_supported_scribe_languages() {
        assert_eq!(normalize_language("en-US"), Some("eng".to_string()));
        assert_eq!(normalize_language("zh-Hant"), Some("zho".to_string()));
        assert_eq!(normalize_language("jw"), Some("jav".to_string()));
        assert_eq!(normalize_language("la"), None);
        assert_eq!(normalize_language("auto"), None);
    }

    #[tokio::test]
    async fn sends_scribe_v2_request_with_audio_event_tags() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/speech-to-text"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "text": "hello (applause)",
                "words": [
                    {"text": "hello", "type": "word"},
                    {"text": " ", "type": "spacing"},
                    {"text": "(applause)", "type": "audio_event"}
                ]
            })))
            .mount(&server)
            .await;

        let transcript = transcribe_at(&server.uri(), "test-secret-key", &[0.0, 0.25], Some("eng"))
            .await
            .unwrap();
        assert_eq!(
            transcript.finish(&AppSettings::default()),
            "hello (applause)"
        );

        let requests = server.received_requests().await.unwrap();
        let request = &requests[0];
        assert_eq!(
            request.headers.get("xi-api-key").unwrap().to_str().unwrap(),
            "test-secret-key"
        );
        let body = String::from_utf8_lossy(&request.body);
        assert!(body.contains("RIFF"));
        assert!(body.contains("name=\"model_id\""));
        assert!(body.contains("scribe_v2"));
        assert!(body.contains("name=\"tag_audio_events\""));
        assert!(body.contains("true"));
        assert!(body.contains("name=\"language_code\""));
        assert!(body.contains("eng"));
    }

    #[tokio::test]
    async fn rejects_missing_key_without_sending_a_request() {
        let server = MockServer::start().await;
        let error = transcribe_at(&server.uri(), " ", &[0.0], None)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("API key is required"));
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn keeps_api_key_out_of_http_errors() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/speech-to-text"))
            .respond_with(ResponseTemplate::new(401).set_body_string("test-secret-key"))
            .mount(&server)
            .await;

        let error = transcribe_at(&server.uri(), "test-secret-key", &[0.0], None)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("HTTP 401"));
        assert!(!error.contains("test-secret-key"));
    }
}
