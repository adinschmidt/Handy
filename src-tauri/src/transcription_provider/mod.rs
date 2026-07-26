mod codex;
mod elevenlabs;

use crate::managers::model::ModelManager;
use crate::managers::transcription::{post_process_transcription_text, TranscriptionManager};
use crate::settings::{AppSettings, TranscriptionProvider};
use anyhow::{anyhow, Context, Result};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Manager};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TranscriptionMode {
    PreferActiveStream,
    BatchOnly,
}

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Deliberately generous: a multi-minute dictation uploaded over a slow link
/// can legitimately take minutes end to end. Past this a request is wedged
/// rather than slow, and an error serves the user better than waiting.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(310);

static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

/// One client shared by every provider and every dictation. The client owns the
/// connection pool and TLS session cache, so building a fresh one per request
/// pays a full handshake on each dictation.
pub(super) fn http_client() -> Result<&'static reqwest::Client> {
    if let Some(client) = HTTP_CLIENT.get() {
        return Ok(client);
    }
    let client = reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
        .context("Failed to create the transcription HTTP client")?;
    Ok(HTTP_CLIENT.get_or_init(|| client))
}

/// Private-use codepoints that bracket a protected span. Text cleanup only
/// rewrites words and punctuation, so it leaves these untouched.
fn protection_marker(index: usize) -> String {
    format!("\u{e000}{index}\u{e001}")
}

/// Raw text from a cloud provider, plus any substrings that must reach the user
/// verbatim. Providers hand this back instead of a finished `String` so that
/// post-processing runs in exactly one place for every cloud target.
#[derive(Debug)]
pub(crate) struct ProviderTranscript {
    text: String,
    protected: Vec<String>,
    appended: Vec<String>,
}

impl ProviderTranscript {
    /// Output with nothing to shield from text cleanup.
    pub(crate) fn plain(text: String) -> Self {
        Self {
            text,
            protected: Vec::new(),
            appended: Vec::new(),
        }
    }

    /// Swap the first occurrence of `span` for a marker that survives cleanup.
    /// A span the provider reported but that isn't present in the text verbatim
    /// can't be located, so it is appended after cleanup instead of dropped.
    pub(crate) fn protect(&mut self, span: String) {
        let marker = protection_marker(self.protected.len());
        if self.text.contains(&span) {
            self.text = self.text.replacen(&span, &marker, 1);
            self.protected.push(span);
        } else {
            self.appended.push(span);
        }
    }

    /// Run text cleanup over the protected text, then restore the spans.
    pub(crate) fn finish(self, settings: &AppSettings) -> String {
        let mut cleaned = post_process_transcription_text(self.text, settings, false);
        for (index, span) in self.protected.into_iter().enumerate() {
            cleaned = cleaned.replace(&protection_marker(index), &span);
        }
        for span in self.appended {
            if !cleaned.is_empty() {
                cleaned.push(' ');
            }
            cleaned.push_str(&span);
        }
        cleaned
    }
}

pub fn is_cloud_provider(settings: &AppSettings) -> bool {
    settings.selected_transcription_provider != TranscriptionProvider::Local
}

pub fn supports_streaming(app: &AppHandle, settings: &AppSettings) -> bool {
    if is_cloud_provider(settings) {
        return false;
    }
    app.state::<Arc<ModelManager>>()
        .get_model_info(&settings.selected_model)
        .map(|model| model.supports_streaming)
        .unwrap_or(false)
}

pub fn prepare_local_model(app: &AppHandle, settings: &AppSettings) {
    if settings.selected_transcription_provider == TranscriptionProvider::Local {
        app.state::<Arc<TranscriptionManager>>()
            .initiate_model_load();
    }
}

async fn transcribe_cloud(
    provider: TranscriptionProvider,
    settings: &AppSettings,
    samples: &[f32],
) -> Result<ProviderTranscript> {
    match provider {
        TranscriptionProvider::CodexAsr => {
            let language = codex::normalize_language(&settings.selected_language);
            codex::transcribe(
                &settings.codex_asr_base_url,
                settings.transcription_api_key(provider),
                samples,
                language.as_deref(),
            )
            .await
        }
        TranscriptionProvider::ElevenlabsScribe => {
            let language = elevenlabs::normalize_language(&settings.selected_language);
            let api_key = settings.transcription_api_key(provider).unwrap_or_default();
            elevenlabs::transcribe(
                api_key,
                samples,
                language.as_deref(),
                settings.elevenlabs_audio_events,
            )
            .await
        }
        TranscriptionProvider::Local => Err(anyhow!("Local is not a cloud transcription provider")),
    }
}

pub async fn transcribe_current_target(
    app: &AppHandle,
    settings: AppSettings,
    samples: Vec<f32>,
    mode: TranscriptionMode,
) -> Result<String> {
    // The provider is resolved here, when the audio is complete, rather than
    // when recording started. Switching providers mid-dictation is therefore
    // honoured by the transcription that follows, at the cost of a plan
    // (VAD policy, overlay style) that was chosen for the previous provider.
    match settings.selected_transcription_provider {
        TranscriptionProvider::Local => {
            let manager = Arc::clone(&app.state::<Arc<TranscriptionManager>>());
            if mode == TranscriptionMode::BatchOnly {
                manager.initiate_model_load();
            }
            tauri::async_runtime::spawn_blocking(move || {
                if mode == TranscriptionMode::PreferActiveStream {
                    match manager.finalize_stream() {
                        Ok(Some(text)) if !text.trim().is_empty() => Ok(text),
                        Ok(_) => manager.transcribe(samples),
                        Err(err) => Err(err),
                    }
                } else {
                    manager.transcribe(samples)
                }
            })
            .await
            .map_err(|err| anyhow!("Transcription task panicked: {}", err))?
        }
        provider => {
            // Cloud targets never consume a live stream, but one may still be
            // running: the dictation can have started while Local was selected
            // with a streaming model. Tear it down so its worker doesn't leak
            // and block the next `start_stream`.
            app.state::<Arc<TranscriptionManager>>().cancel_stream();
            let transcript = transcribe_cloud(provider, &settings, &samples).await?;
            Ok(transcript.finish(&settings))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ProviderTranscript;
    use crate::settings::AppSettings;

    #[test]
    fn restores_protected_spans_after_cleanup() {
        let settings = AppSettings {
            app_language: "en".to_string(),
            custom_words: vec!["Handy".to_string()],
            ..AppSettings::default()
        };

        let mut transcript = ProviderTranscript::plain("handy um (applause)".to_string());
        transcript.protect("(applause)".to_string());

        assert_eq!(transcript.finish(&settings), "Handy (applause)");
    }

    #[test]
    fn appends_spans_that_are_absent_from_the_text() {
        let mut transcript =
            ProviderTranscript::plain("A complete sentence with punctuation.".to_string());
        transcript.protect("(applause)".to_string());

        assert_eq!(
            transcript.finish(&AppSettings::default()),
            "A complete sentence with punctuation. (applause)"
        );
    }

    #[test]
    fn plain_output_is_only_post_processed() {
        let settings = AppSettings {
            app_language: "en".to_string(),
            custom_words: vec!["Handy".to_string()],
            ..AppSettings::default()
        };

        let transcript = ProviderTranscript::plain("handy um".to_string());
        assert_eq!(transcript.finish(&settings), "Handy");
    }
}
