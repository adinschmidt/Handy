mod codex;

use crate::managers::model::ModelManager;
use crate::managers::transcription::{post_process_transcription_text, TranscriptionManager};
use crate::settings::{AppSettings, TranscriptionProvider};
use anyhow::{anyhow, Result};
use std::sync::Arc;
use tauri::{AppHandle, Manager};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TranscriptionMode {
    PreferActiveStream,
    BatchOnly,
}

pub fn normalize_language(language: &str) -> Option<String> {
    if language == "auto" {
        return None;
    }
    if language == "zh" || language.starts_with("zh-") {
        return Some("zh".to_string());
    }
    Some(language.split('-').next().unwrap_or(language).to_string())
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

pub async fn transcribe_current_target(
    app: &AppHandle,
    settings: AppSettings,
    samples: Vec<f32>,
    mode: TranscriptionMode,
) -> Result<String> {
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
        TranscriptionProvider::CodexAsr => {
            app.state::<Arc<TranscriptionManager>>().cancel_stream();
            let language = normalize_language(&settings.selected_language);
            let raw =
                codex::transcribe(&settings.codex_asr_base_url, &samples, language.as_deref())
                    .await?;
            Ok(post_process_transcription_text(raw, &settings, false))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_language;

    #[test]
    fn normalizes_provider_language_codes() {
        assert_eq!(normalize_language("auto"), None);
        assert_eq!(normalize_language("zh-Hant"), Some("zh".to_string()));
        assert_eq!(normalize_language("en-US"), Some("en".to_string()));
        assert_eq!(normalize_language("yue"), Some("yue".to_string()));
    }
}
