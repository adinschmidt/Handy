use crate::managers::transcription::TranscriptionManager;
use crate::settings::{get_settings, write_settings, ModelUnloadTimeout, TranscriptionProvider};
use serde::Serialize;
use specta::Type;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Serialize, Type)]
pub struct ModelLoadStatus {
    is_loaded: bool,
    current_model: Option<String>,
}

#[tauri::command]
#[specta::specta]
pub fn set_model_unload_timeout(app: AppHandle, timeout: ModelUnloadTimeout) {
    let mut settings = get_settings(&app);
    settings.model_unload_timeout = timeout;
    write_settings(&app, settings);
}

#[tauri::command]
#[specta::specta]
pub fn get_model_load_status(
    transcription_manager: State<TranscriptionManager>,
) -> Result<ModelLoadStatus, String> {
    Ok(ModelLoadStatus {
        is_loaded: transcription_manager.is_model_loaded(),
        current_model: transcription_manager.get_current_model(),
    })
}

#[tauri::command]
#[specta::specta]
pub fn unload_model_manually(
    transcription_manager: State<TranscriptionManager>,
) -> Result<(), String> {
    transcription_manager
        .unload_model()
        .map_err(|e| format!("Failed to unload model: {}", e))
}

#[tauri::command]
#[specta::specta]
pub fn set_transcription_provider(
    app: AppHandle,
    provider: TranscriptionProvider,
) -> Result<(), String> {
    let mut settings = get_settings(&app);
    // Codex ASR authentication is optional, so only ElevenLabs is gated here.
    if provider == TranscriptionProvider::ElevenlabsScribe
        && settings.transcription_api_key(provider).is_none()
    {
        return Err("ElevenLabs API key is required".to_string());
    }
    settings.selected_transcription_provider = provider;
    write_settings(&app, settings.clone());

    let manager = app.state::<Arc<TranscriptionManager>>();
    match provider {
        TranscriptionProvider::Local => {
            if !settings.selected_model.is_empty() {
                manager.initiate_model_load();
            }
        }
        TranscriptionProvider::CodexAsr | TranscriptionProvider::ElevenlabsScribe => {
            manager
                .unload_model()
                .map_err(|err| format!("Failed to unload local model: {}", err))?;
        }
    }

    let _ = app.emit("transcription-provider-changed", provider);
    crate::tray::update_tray_menu(&app);
    Ok(())
}

fn normalize_codex_base_url(base_url: &str) -> Result<String, String> {
    let normalized = base_url.trim().trim_end_matches('/').to_string();
    if normalized.is_empty() {
        return Err("Codex ASR base URL cannot be empty".to_string());
    }
    let parsed =
        reqwest::Url::parse(&normalized).map_err(|_| "Invalid Codex ASR URL".to_string())?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err("Codex ASR URL must use http or https".to_string());
    }
    // A base path is fine -- the server may sit behind a reverse proxy. A query
    // or fragment is not: the transcription path is appended to this value, so
    // either one would produce a malformed endpoint. Credentials belong in the
    // API key setting, where they are stored redacted.
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("Codex ASR URL must not embed credentials; use the API key field".to_string());
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err("Codex ASR URL must not contain a query or fragment".to_string());
    }
    Ok(normalized)
}

#[tauri::command]
#[specta::specta]
pub fn change_codex_asr_base_url(app: AppHandle, base_url: String) -> Result<(), String> {
    let normalized = normalize_codex_base_url(&base_url)?;
    let mut settings = get_settings(&app);
    settings.codex_asr_base_url = normalized;
    write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_transcription_api_key(
    app: AppHandle,
    provider: TranscriptionProvider,
    api_key: String,
) -> Result<(), String> {
    let provider_id = provider
        .api_key_id()
        .ok_or_else(|| "This transcription provider does not use an API key".to_string())?;
    let mut settings = get_settings(&app);
    settings
        .transcription_api_keys
        .insert(provider_id.to_string(), api_key.trim().to_string());
    write_settings(&app, settings);
    crate::tray::update_tray_menu(&app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::normalize_codex_base_url;

    #[test]
    fn validates_and_normalizes_codex_urls() {
        assert_eq!(
            normalize_codex_base_url(" http://127.0.0.1:8788/ ").unwrap(),
            "http://127.0.0.1:8788"
        );
        // A reverse-proxied server under a base path is a supported setup.
        assert_eq!(
            normalize_codex_base_url("https://asr.example.com/codex/").unwrap(),
            "https://asr.example.com/codex"
        );
        assert!(normalize_codex_base_url("").is_err());
        assert!(normalize_codex_base_url("file:///tmp/codex").is_err());
        assert!(normalize_codex_base_url("http://user:secret@localhost:8788").is_err());
        assert!(normalize_codex_base_url("http://localhost:8788/path?token=secret").is_err());
    }
}
