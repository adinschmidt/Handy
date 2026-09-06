use crate::audio_feedback::open_output_stream;
use log::warn;
use rodio::{Decoder, OutputStream, Sink, Source};
use serde::Serialize;
use specta::Type;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const STATE_REFRESH_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Clone, Debug, Default, Serialize, Type)]
pub struct HistoryAudioPlaybackState {
    pub file_name: Option<String>,
    pub is_playing: bool,
    pub position_seconds: f64,
    pub duration_seconds: f64,
}

enum PlaybackCommand {
    Play {
        file_name: String,
        path: PathBuf,
        output_device: Option<String>,
        reply: mpsc::Sender<Result<HistoryAudioPlaybackState, String>>,
    },
    Pause {
        file_name: String,
        reply: mpsc::Sender<Result<HistoryAudioPlaybackState, String>>,
    },
    Seek {
        file_name: String,
        position: Duration,
        reply: mpsc::Sender<Result<HistoryAudioPlaybackState, String>>,
    },
    Stop {
        file_name: String,
        reply: Option<mpsc::Sender<Result<HistoryAudioPlaybackState, String>>>,
    },
    Shutdown,
}

struct PlaybackSession {
    file_name: String,
    path: PathBuf,
    duration: Duration,
    _stream: OutputStream,
    sink: Sink,
}

pub struct HistoryAudioManager {
    tx: mpsc::Sender<PlaybackCommand>,
    state: Arc<Mutex<HistoryAudioPlaybackState>>,
    worker: Mutex<Option<thread::JoinHandle<()>>>,
}

impl HistoryAudioManager {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        let state = Arc::new(Mutex::new(HistoryAudioPlaybackState::default()));
        let worker_state = Arc::clone(&state);
        let worker = thread::spawn(move || run_playback_worker(rx, worker_state));

        Self {
            tx,
            state,
            worker: Mutex::new(Some(worker)),
        }
    }

    pub fn play(
        &self,
        file_name: String,
        path: PathBuf,
        output_device: Option<String>,
    ) -> Result<HistoryAudioPlaybackState, String> {
        self.request(|reply| PlaybackCommand::Play {
            file_name,
            path,
            output_device,
            reply,
        })
    }

    pub fn pause(&self, file_name: String) -> Result<HistoryAudioPlaybackState, String> {
        self.request(|reply| PlaybackCommand::Pause { file_name, reply })
    }

    pub fn seek(
        &self,
        file_name: String,
        position_seconds: f64,
    ) -> Result<HistoryAudioPlaybackState, String> {
        if !position_seconds.is_finite() || position_seconds < 0.0 {
            return Err("Playback position must be a non-negative number".to_string());
        }
        let position = Duration::try_from_secs_f64(position_seconds)
            .map_err(|_| "Playback position is out of range".to_string())?;
        self.request(|reply| PlaybackCommand::Seek {
            file_name,
            position,
            reply,
        })
    }

    pub fn stop(&self, file_name: String) -> Result<HistoryAudioPlaybackState, String> {
        self.request(|reply| PlaybackCommand::Stop {
            file_name,
            reply: Some(reply),
        })
    }

    pub fn stop_without_waiting(&self, file_name: String) {
        let _ = self.tx.send(PlaybackCommand::Stop {
            file_name,
            reply: None,
        });
    }

    pub fn state(&self) -> HistoryAudioPlaybackState {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn request<F>(&self, build_command: F) -> Result<HistoryAudioPlaybackState, String>
    where
        F: FnOnce(mpsc::Sender<Result<HistoryAudioPlaybackState, String>>) -> PlaybackCommand,
    {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.tx
            .send(build_command(reply_tx))
            .map_err(|_| "History audio worker is unavailable".to_string())?;
        reply_rx
            .recv_timeout(COMMAND_TIMEOUT)
            .map_err(|_| "Timed out waiting for history audio playback".to_string())?
    }
}

impl Default for HistoryAudioManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for HistoryAudioManager {
    fn drop(&mut self) {
        let _ = self.tx.send(PlaybackCommand::Shutdown);
        if let Some(worker) = self
            .worker
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        {
            if let Err(error) = worker.join() {
                warn!("Failed to join history audio worker: {error:?}");
            }
        }
    }
}

fn run_playback_worker(
    rx: mpsc::Receiver<PlaybackCommand>,
    shared_state: Arc<Mutex<HistoryAudioPlaybackState>>,
) {
    let mut session: Option<PlaybackSession> = None;

    loop {
        match rx.recv_timeout(STATE_REFRESH_INTERVAL) {
            Ok(PlaybackCommand::Play {
                file_name,
                path,
                output_device,
                reply,
            }) => {
                let result = if let Some(active) = session.as_ref() {
                    if active.file_name == file_name && !active.sink.empty() {
                        active.sink.play();
                        Ok(snapshot(active))
                    } else {
                        start_session(file_name, path, output_device).map(|new_session| {
                            let state = snapshot(&new_session);
                            session = Some(new_session);
                            state
                        })
                    }
                } else {
                    start_session(file_name, path, output_device).map(|new_session| {
                        let state = snapshot(&new_session);
                        session = Some(new_session);
                        state
                    })
                };
                if let Ok(state) = &result {
                    set_shared_state(&shared_state, state.clone());
                }
                let _ = reply.send(result);
            }
            Ok(PlaybackCommand::Pause { file_name, reply }) => {
                let result = matching_session(&session, &file_name).map(|active| {
                    active.sink.pause();
                    snapshot(active)
                });
                if let Ok(state) = &result {
                    set_shared_state(&shared_state, state.clone());
                }
                let _ = reply.send(result);
            }
            Ok(PlaybackCommand::Seek {
                file_name,
                position,
                reply,
            }) => {
                let result = matching_session(&session, &file_name).and_then(|active| {
                    if active.sink.empty() {
                        let file = File::open(&active.path).map_err(|err| err.to_string())?;
                        let decoder =
                            Decoder::new(BufReader::new(file)).map_err(|err| err.to_string())?;
                        active.sink.pause();
                        active.sink.append(decoder);
                    }
                    active
                        .sink
                        .try_seek(position.min(active.duration))
                        .map_err(|error| format!("Failed to seek history audio: {error}"))?;
                    Ok(snapshot(active))
                });
                if let Ok(state) = &result {
                    set_shared_state(&shared_state, state.clone());
                }
                let _ = reply.send(result);
            }
            Ok(PlaybackCommand::Stop { file_name, reply }) => {
                let state = if session
                    .as_ref()
                    .is_some_and(|active| active.file_name == file_name)
                {
                    if let Some(active) = session.take() {
                        active.sink.stop();
                    }
                    HistoryAudioPlaybackState::default()
                } else {
                    current_state(&shared_state)
                };
                set_shared_state(&shared_state, state.clone());
                if let Some(reply) = reply {
                    let _ = reply.send(Ok(state));
                }
            }
            Ok(PlaybackCommand::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }

        if let Some(active) = session.as_ref() {
            set_shared_state(&shared_state, snapshot(active));
        }
    }

    if let Some(active) = session {
        active.sink.stop();
    }
    set_shared_state(&shared_state, HistoryAudioPlaybackState::default());
}

fn start_session(
    file_name: String,
    path: PathBuf,
    output_device: Option<String>,
) -> Result<PlaybackSession, String> {
    let file = File::open(&path)
        .map_err(|error| format!("Failed to open history audio '{}': {error}", path.display()))?;
    let decoder = Decoder::new(BufReader::new(file))
        .map_err(|error| format!("Failed to decode history audio: {error}"))?;
    let duration = decoder
        .total_duration()
        .ok_or_else(|| "History audio duration is unavailable".to_string())?;
    let stream = open_output_stream(output_device)
        .map_err(|error| format!("Failed to open audio output: {error}"))?;
    let sink = Sink::connect_new(stream.mixer());
    sink.append(decoder);

    Ok(PlaybackSession {
        file_name,
        path,
        duration,
        _stream: stream,
        sink,
    })
}

fn matching_session<'a>(
    session: &'a Option<PlaybackSession>,
    file_name: &str,
) -> Result<&'a PlaybackSession, String> {
    session
        .as_ref()
        .filter(|active| active.file_name == file_name)
        .ok_or_else(|| "This recording is not the active history audio".to_string())
}

fn snapshot(session: &PlaybackSession) -> HistoryAudioPlaybackState {
    let ended = session.sink.empty();
    let position = if ended {
        session.duration
    } else {
        session.sink.get_pos().min(session.duration)
    };

    HistoryAudioPlaybackState {
        file_name: Some(session.file_name.clone()),
        is_playing: !ended && !session.sink.is_paused(),
        position_seconds: position.as_secs_f64(),
        duration_seconds: session.duration.as_secs_f64(),
    }
}

fn set_shared_state(
    shared_state: &Mutex<HistoryAudioPlaybackState>,
    state: HistoryAudioPlaybackState,
) {
    *shared_state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = state;
}

fn current_state(shared_state: &Mutex<HistoryAudioPlaybackState>) -> HistoryAudioPlaybackState {
    shared_state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_toolkit::save_wav_file;

    #[test]
    fn invalid_seek_does_not_panic_or_enqueue_playback() {
        let manager = HistoryAudioManager::new();
        for position in [-1.0, f64::NAN, f64::INFINITY, f64::MAX] {
            assert!(manager.seek("silent.wav".into(), position).is_err());
        }
    }

    #[test]
    #[ignore = "requires an audio output device"]
    fn native_playback_controls_a_real_wav() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("silent.wav");
        save_wav_file(&path, &vec![0.0; 16_000]).unwrap();

        let manager = HistoryAudioManager::new();
        let started = match manager.play("silent.wav".to_string(), path.clone(), None) {
            Ok(state) => state,
            Err(error) => panic!("native playback failed: {error}"),
        };

        assert_eq!(started.file_name.as_deref(), Some("silent.wav"));
        assert!(started.is_playing);
        assert!((0.9..=1.1).contains(&started.duration_seconds));

        let paused = manager.pause("silent.wav".to_string()).unwrap();
        assert!(!paused.is_playing);

        let sought = manager.seek("silent.wav".to_string(), 0.5).unwrap();
        assert!((0.45..=0.55).contains(&sought.position_seconds));

        manager.play("silent.wav".to_string(), path, None).unwrap();
        thread::sleep(Duration::from_millis(800));
        assert!(!manager.state().is_playing);
        let sought_after_end = manager.seek("silent.wav".to_string(), 0.25).unwrap();
        assert!(!sought_after_end.is_playing);
        assert!((0.20..=0.30).contains(&sought_after_end.position_seconds));

        let stopped = manager.stop("silent.wav".to_string()).unwrap();
        assert_eq!(stopped.file_name, None);
        assert!(!stopped.is_playing);
    }
}
