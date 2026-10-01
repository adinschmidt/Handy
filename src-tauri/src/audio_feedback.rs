#[cfg(target_os = "linux")]
use crate::audio_toolkit::audio::pulse::{self, PulsePlayback, PulseSink};
use crate::audio_toolkit::{list_output_devices, OutputDevice};
use crate::settings::SoundTheme;
use crate::settings::{self, AppSettings};
use log::{debug, error, warn};
use rodio::mixer::Mixer;
use rodio::{OutputStreamBuilder, Source};
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

/// Time a feedback sound may take beyond its own length before the wait for
/// it gives up.
const FEEDBACK_SLACK: Duration = Duration::from_secs(2);
/// Wait limit for a sound whose length the decoder cannot report.
const FEEDBACK_UNKNOWN_LENGTH: Duration = Duration::from_secs(10);
/// The start sound holds up recording, so notice its end promptly.
const FEEDBACK_POLL_INTERVAL: Duration = Duration::from_millis(5);

/// An open output that sounds are mixed into.
pub(crate) enum OutputStream {
    Rodio(rodio::OutputStream),
    /// A rodio mixer drained into a sound server playback stream.
    #[cfg(target_os = "linux")]
    Pulse {
        mixer: Mixer,
        _playback: PulsePlayback,
    },
}

impl OutputStream {
    pub(crate) fn mixer(&self) -> &Mixer {
        match self {
            OutputStream::Rodio(stream) => stream.mixer(),
            #[cfg(target_os = "linux")]
            OutputStream::Pulse { mixer, .. } => mixer,
        }
    }
}

pub enum SoundType {
    Start,
    Stop,
}

fn resolve_sound_path(
    app: &AppHandle,
    settings: &AppSettings,
    sound_type: SoundType,
) -> Option<PathBuf> {
    let sound_file = get_sound_path(settings, sound_type);
    let base_dir = get_sound_base_dir(settings);
    match base_dir {
        tauri::path::BaseDirectory::AppData => {
            crate::portable::resolve_app_data(app, &sound_file).ok()
        }
        _ => app.path().resolve(&sound_file, base_dir).ok(),
    }
}

fn get_sound_path(settings: &AppSettings, sound_type: SoundType) -> String {
    match (settings.sound_theme, sound_type) {
        (SoundTheme::Custom, SoundType::Start) => "custom_start.wav".to_string(),
        (SoundTheme::Custom, SoundType::Stop) => "custom_stop.wav".to_string(),
        (_, SoundType::Start) => settings.sound_theme.to_start_path(),
        (_, SoundType::Stop) => settings.sound_theme.to_stop_path(),
    }
}

fn get_sound_base_dir(settings: &AppSettings) -> tauri::path::BaseDirectory {
    match settings.sound_theme {
        SoundTheme::Custom => tauri::path::BaseDirectory::AppData,
        _ => tauri::path::BaseDirectory::Resource,
    }
}

pub fn play_feedback_sound(app: &AppHandle, sound_type: SoundType) {
    let settings = settings::get_settings(app);
    if !settings.audio_feedback {
        return;
    }
    if let Some(path) = resolve_sound_path(app, &settings, sound_type) {
        play_sound_async(app, path);
    }
}

pub fn play_feedback_sound_blocking(app: &AppHandle, sound_type: SoundType) {
    let settings = settings::get_settings(app);
    if !settings.audio_feedback {
        return;
    }
    if let Some(path) = resolve_sound_path(app, &settings, sound_type) {
        play_sound_blocking(app, &path);
    }
}

pub fn play_test_sound(app: &AppHandle, sound_type: SoundType) {
    let settings = settings::get_settings(app);
    if let Some(path) = resolve_sound_path(app, &settings, sound_type) {
        play_sound_blocking(app, &path);
    }
}

fn play_sound_async(app: &AppHandle, path: PathBuf) {
    let app_handle = app.clone();
    thread::spawn(move || {
        if let Err(e) = play_sound_at_path(&app_handle, path.as_path()) {
            error!("Failed to play sound '{}': {}", path.display(), e);
        }
    });
}

fn play_sound_blocking(app: &AppHandle, path: &Path) {
    if let Err(e) = play_sound_at_path(app, path) {
        error!("Failed to play sound '{}': {}", path.display(), e);
    }
}

fn play_sound_at_path(app: &AppHandle, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let settings = settings::get_settings(app);
    let volume = settings.audio_feedback_volume;
    let selected_device = settings.selected_output_device.clone();
    play_audio_file(path, selected_device, volume)
}

fn play_audio_file(
    path: &std::path::Path,
    selected_device: Option<String>,
    volume: f32,
) -> Result<(), Box<dyn std::error::Error>> {
    let stream_handle = open_output_stream(selected_device)?;
    let mixer = stream_handle.mixer();

    let file = File::open(path)?;
    let source = rodio::Decoder::new(BufReader::new(file))?;
    // A wedged device or stalled sound server never finishes the sound, and
    // the start sound holds up recording, so bound the wait by its length.
    let limit = source.total_duration().unwrap_or(FEEDBACK_UNKNOWN_LENGTH) + FEEDBACK_SLACK;

    let sink = rodio::Sink::connect_new(mixer);
    sink.set_volume(volume);
    sink.append(source);
    let started = Instant::now();
    while !sink.empty() {
        if started.elapsed() > limit {
            warn!("Feedback sound did not finish within {limit:?}; closing the output");
            break;
        }
        thread::sleep(FEEDBACK_POLL_INTERVAL);
    }

    Ok(())
}

pub(crate) fn open_output_stream(
    selected_device: Option<String>,
) -> Result<OutputStream, Box<dyn std::error::Error>> {
    let device = match selected_device.filter(|name| name != "Default") {
        Some(device_name) => {
            let device = list_output_devices()?
                .into_iter()
                .find(|device| device.name == device_name)
                .map(|device| device.device);
            if device.is_none() {
                warn!("Device '{}' not found, using default device", device_name);
            }
            device
        }
        None => None,
    };

    let stream_builder = match device {
        Some(OutputDevice::Cpal(device)) => OutputStreamBuilder::from_device(device)?,
        #[cfg(target_os = "linux")]
        Some(OutputDevice::Pulse(sink)) => return open_pulse_output(&sink),
        None => {
            debug!("Using default device");
            OutputStreamBuilder::from_default_device()?
        }
    };

    Ok(OutputStream::Rodio(stream_builder.open_stream()?))
}

/// Mixes sounds the way rodio's own output does, but drains the mixer into a
/// sound server stream instead of opening the card through ALSA.
#[cfg(target_os = "linux")]
fn open_pulse_output(sink: &PulseSink) -> Result<OutputStream, Box<dyn std::error::Error>> {
    let format = pulse::playback_format(&sink.name)?;
    let (mixer, mut source) =
        rodio::mixer::mixer(u16::try_from(format.channels)?, format.sample_rate);
    let playback = pulse::open_playback(&format, move |out| {
        out.fill_with(|| source.next().unwrap_or(0.0));
    })?;
    debug!("Using sound server sink {:?}", sink.description);

    Ok(OutputStream::Pulse {
        mixer,
        _playback: playback,
    })
}

#[cfg(test)]
mod tests {
    use super::play_audio_file;
    use crate::audio_toolkit::{list_output_devices, save_wav_file};
    use std::time::{Duration, Instant};

    /// Plays a quiet 0.3 s tone through the feedback path on the output whose
    /// name contains `HANDY_TEST_OUTPUT`.
    #[test]
    #[ignore = "plays a sound on a live output device"]
    fn live_feedback_playback() {
        let wanted = std::env::var("HANDY_TEST_OUTPUT").expect("set HANDY_TEST_OUTPUT");
        let name = list_output_devices()
            .unwrap()
            .into_iter()
            .map(|device| device.name)
            .find(|name| name.contains(wanted.as_str()))
            .expect("requested output should exist");

        let tone: Vec<f32> = (0..4_800)
            .map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / 16_000.0).sin() * 0.1)
            .collect();
        let path = std::env::temp_dir().join("handy-live-feedback-test.wav");
        save_wav_file(&path, &tone).unwrap();

        let started = Instant::now();
        play_audio_file(&path, Some(name.clone()), 0.2).unwrap();
        let elapsed = started.elapsed();
        let _ = std::fs::remove_file(&path);

        println!("played on {name:?} in {elapsed:?}");
        assert!(
            elapsed >= Duration::from_millis(300),
            "returned before the tone ended"
        );
        assert!(
            elapsed < Duration::from_secs(2),
            "playback or drain stalled"
        );
    }
}
