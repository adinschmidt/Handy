use crate::actions::process_transcription_output;
use crate::managers::history::{HistoryManager, PaginatedHistory};
use crate::managers::history_audio::{HistoryAudioManager, HistoryAudioPlaybackState};
use crate::transcription_provider::{self, TranscriptionMode};
use std::path::{Component, Path};
use std::sync::Arc;
use tauri::{AppHandle, State};

#[tauri::command]
#[specta::specta]
pub async fn get_history_entries(
    _app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
    cursor: Option<i64>,
    limit: Option<usize>,
) -> Result<PaginatedHistory, String> {
    history_manager
        .get_history_entries(cursor, limit)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn toggle_history_entry_saved(
    _app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
    id: i64,
) -> Result<(), String> {
    history_manager
        .toggle_saved_status(id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn get_audio_file_path(
    _app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
    file_name: String,
) -> Result<String, String> {
    validate_recording_file_name(&file_name)?;
    let path = history_manager.get_audio_file_path(&file_name);
    path.to_str()
        .ok_or_else(|| "Invalid file path".to_string())
        .map(|s| s.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn play_history_audio(
    app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
    audio_manager: State<'_, Arc<HistoryAudioManager>>,
    file_name: String,
) -> Result<HistoryAudioPlaybackState, String> {
    validate_recording_file_name(&file_name)?;
    let path = history_manager.get_audio_file_path(&file_name);
    if !path.is_file() {
        return Err(format!("Recording '{file_name}' does not exist"));
    }
    let output_device = crate::settings::get_settings(&app).selected_output_device;
    let audio_manager = Arc::clone(audio_manager.inner());
    tauri::async_runtime::spawn_blocking(move || audio_manager.play(file_name, path, output_device))
        .await
        .map_err(|err| err.to_string())?
}

#[tauri::command]
#[specta::specta]
pub async fn pause_history_audio(
    audio_manager: State<'_, Arc<HistoryAudioManager>>,
    file_name: String,
) -> Result<HistoryAudioPlaybackState, String> {
    validate_recording_file_name(&file_name)?;
    let audio_manager = Arc::clone(audio_manager.inner());
    tauri::async_runtime::spawn_blocking(move || audio_manager.pause(file_name))
        .await
        .map_err(|err| err.to_string())?
}

#[tauri::command]
#[specta::specta]
pub async fn seek_history_audio(
    audio_manager: State<'_, Arc<HistoryAudioManager>>,
    file_name: String,
    position_seconds: f64,
) -> Result<HistoryAudioPlaybackState, String> {
    validate_recording_file_name(&file_name)?;
    let audio_manager = Arc::clone(audio_manager.inner());
    tauri::async_runtime::spawn_blocking(move || audio_manager.seek(file_name, position_seconds))
        .await
        .map_err(|err| err.to_string())?
}

#[tauri::command]
#[specta::specta]
pub async fn stop_history_audio(
    audio_manager: State<'_, Arc<HistoryAudioManager>>,
    file_name: String,
) -> Result<HistoryAudioPlaybackState, String> {
    validate_recording_file_name(&file_name)?;
    let audio_manager = Arc::clone(audio_manager.inner());
    tauri::async_runtime::spawn_blocking(move || audio_manager.stop(file_name))
        .await
        .map_err(|err| err.to_string())?
}

#[tauri::command]
#[specta::specta]
pub fn get_history_audio_playback_state(
    audio_manager: State<'_, Arc<HistoryAudioManager>>,
) -> HistoryAudioPlaybackState {
    audio_manager.state()
}

#[tauri::command]
#[specta::specta]
pub async fn delete_history_entry(
    audio_manager: State<'_, Arc<HistoryAudioManager>>,
    _app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
    id: i64,
) -> Result<(), String> {
    if let Some(entry) = history_manager
        .get_entry_by_id(id)
        .await
        .map_err(|e| e.to_string())?
    {
        audio_manager.stop_without_waiting(entry.file_name);
    }
    history_manager
        .delete_entry(id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn retry_history_entry_transcription(
    app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
    id: i64,
) -> Result<(), String> {
    let entry = history_manager
        .get_entry_by_id(id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("History entry {} not found", id))?;

    let audio_path = history_manager.get_audio_file_path(&entry.file_name);
    let samples = crate::audio_toolkit::read_wav_samples(&audio_path)
        .map_err(|e| format!("Failed to load audio: {}", e))?;

    if samples.is_empty() {
        return Err("Recording has no audio samples".to_string());
    }

    let settings = crate::settings::get_settings(&app);
    let transcription = transcription_provider::transcribe_current_target(
        &app,
        settings,
        samples,
        TranscriptionMode::BatchOnly,
    )
    .await
    .map_err(|e| e.to_string())?;

    if transcription.is_empty() {
        return Err("Recording contains no speech".to_string());
    }

    let processed =
        process_transcription_output(&app, &transcription, entry.post_process_requested).await;
    history_manager
        .update_transcription(
            id,
            transcription,
            processed.post_processed_text,
            processed.post_process_prompt,
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn update_history_limit(
    app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
    limit: usize,
) -> Result<(), String> {
    let mut settings = crate::settings::get_settings(&app);
    settings.history_limit = limit;
    crate::settings::write_settings(&app, settings);

    history_manager
        .cleanup_old_entries()
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn update_recording_retention_period(
    app: AppHandle,
    history_manager: State<'_, Arc<HistoryManager>>,
    period: String,
) -> Result<(), String> {
    use crate::settings::RecordingRetentionPeriod;

    let retention_period = match period.as_str() {
        "never" => RecordingRetentionPeriod::Never,
        "preserve_limit" => RecordingRetentionPeriod::PreserveLimit,
        "days3" => RecordingRetentionPeriod::Days3,
        "weeks2" => RecordingRetentionPeriod::Weeks2,
        "months3" => RecordingRetentionPeriod::Months3,
        _ => return Err(format!("Invalid retention period: {}", period)),
    };

    let mut settings = crate::settings::get_settings(&app);
    settings.recording_retention_period = retention_period;
    crate::settings::write_settings(&app, settings);

    history_manager
        .cleanup_old_entries()
        .map_err(|e| e.to_string())?;

    Ok(())
}

fn validate_recording_file_name(file_name: &str) -> Result<(), String> {
    let path = Path::new(file_name);
    let mut components = path.components();
    let is_single_normal_component =
        matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none();
    if !is_single_normal_component || path.extension().and_then(|ext| ext.to_str()) != Some("wav") {
        return Err("Invalid recording file name".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_recording_file_name;

    #[test]
    fn recording_file_name_validation_blocks_path_traversal() {
        assert!(validate_recording_file_name("handy-123.wav").is_ok());
        for invalid in [
            "../handy-123.wav",
            "recordings/handy-123.wav",
            "/tmp/handy-123.wav",
            "handy-123.mp3",
            "",
        ] {
            assert!(validate_recording_file_name(invalid).is_err(), "{invalid}");
        }
    }
}
