//! Microphone capture through the PulseAudio protocol, which PipeWire also
//! serves (pipewire-pulse).
//!
//! cpal's ALSA host exposes each sound card as `plughw:N`. Selecting one opens
//! the hardware directly: PipeWire loses the device while Handy holds it, the
//! card disappears from the list whenever PipeWire is using it, and the device
//! is reclocked to whatever rate Handy asks for. Capturing a named server
//! source instead shares the device with every other application and leaves
//! the system default source untouched.

use std::ffi::CString;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Mutex;
use std::time::Duration;

use futures::executor::block_on;
use pulseaudio::{protocol, Client, ClientError, RecordStream};

/// `Client::from_env` performs a blocking handshake with no socket timeout.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);
/// How often an open capture checks that the server still has its stream.
const WATCHDOG_INTERVAL: Duration = Duration::from_secs(1);
/// Requested delivery size. The server default for record streams can be
/// seconds long, which would delay the first samples of every recording.
const FRAGMENT_DURATION_MS: u64 = 20;
const BYTES_PER_SAMPLE: usize = 4;

/// One connection shared by enumeration and capture, re-established after the
/// server goes away.
static CLIENT: Mutex<Option<Client>> = Mutex::new(None);

/// A capture source published by the sound server.
#[derive(Clone, Debug)]
pub struct PulseSource {
    /// Server-side name used to route the stream, e.g.
    /// `alsa_input.usb-Blue_Microphones_Yeti_Nano_…analog-stereo`.
    pub name: String,
    /// Human-readable name shown in the microphone list.
    pub description: String,
    pub channels: u16,
    pub is_default: bool,
}

/// The source's current native layout, looked up at open time so the server
/// only converts the sample format.
pub struct CaptureFormat {
    pub channels: usize,
    pub sample_rate: u32,
    source_index: u32,
    channel_map: protocol::ChannelMap,
}

/// Keeps a record stream alive; the server deletes it once this is dropped.
pub struct PulseCapture {
    _stream: RecordStream,
    _stop_watchdog: mpsc::Sender<()>,
}

enum CallError {
    Timeout,
    Client(ClientError),
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CallError::Timeout => f.write_str("sound server did not respond"),
            CallError::Client(e) => write!(f, "sound server error: {e}"),
        }
    }
}

/// Whether a sound server socket exists. Socket activation creates it before
/// the server starts, so its presence means capture belongs to the server.
pub fn server_expected() -> bool {
    pulseaudio::socket_path_from_env().is_some()
}

/// Lists microphone sources, excluding the monitors of output devices.
pub fn list_sources() -> Result<Vec<PulseSource>, String> {
    let (default_name, sources) = request(|client| {
        let server = block_on(client.server_info())?;
        let sources = block_on(client.list_sources())?;
        Ok((server.default_source_name, sources))
    })?;

    Ok(sources
        .into_iter()
        .filter(|source| source.monitor_of_sink_index.is_none())
        .map(|source| {
            let name = source.name.to_string_lossy().into_owned();
            PulseSource {
                description: source
                    .description
                    .as_ref()
                    .map(|description| description.to_string_lossy().into_owned())
                    .unwrap_or_else(|| name.clone()),
                channels: u16::from(source.sample_spec.channels),
                is_default: default_name.as_ref() == Some(&source.name),
                name,
            }
        })
        .collect())
}

pub fn capture_format(source_name: &str) -> Result<CaptureFormat, String> {
    let name = CString::new(source_name).map_err(|e| format!("Invalid source name: {e}"))?;
    let info = request(move |client| block_on(client.source_info_by_name(name.clone())))?;
    Ok(CaptureFormat {
        channels: usize::from(info.sample_spec.channels),
        sample_rate: info.sample_spec.sample_rate,
        source_index: info.index,
        channel_map: info.channel_map,
    })
}

/// Starts capturing interleaved f32 frames from the source.
///
/// `on_samples` runs on the client's I/O thread and always receives whole
/// frames. `on_failure` runs once if the server or the stream goes away.
pub fn open_capture(
    format: &CaptureFormat,
    mut on_samples: impl FnMut(&[f32]) + Send + 'static,
    on_failure: impl FnOnce() + Send + 'static,
) -> Result<PulseCapture, String> {
    let frame_bytes = format.channels * BYTES_PER_SAMPLE;
    let fragment_frames = u64::from(format.sample_rate) * FRAGMENT_DURATION_MS / 1000;
    let fragment_size = u32::try_from(fragment_frames * frame_bytes as u64).unwrap_or(u32::MAX);

    let mut props = protocol::Props::new();
    props.set(protocol::Prop::MediaName, c"Microphone");

    let params = protocol::RecordStreamParams {
        sample_spec: protocol::SampleSpec {
            format: protocol::SampleFormat::Float32Le,
            channels: u8::try_from(format.channels).unwrap_or(u8::MAX),
            sample_rate: format.sample_rate,
        },
        channel_map: format.channel_map,
        source_index: Some(format.source_index),
        buffer_attr: protocol::stream::BufferAttr {
            fragment_size,
            ..Default::default()
        },
        flags: protocol::stream::StreamFlags {
            adjust_latency: true,
            ..Default::default()
        },
        props,
        ..Default::default()
    };

    let mut decoder = FrameDecoder::new(frame_bytes);
    let callback = move |data: &[u8]| {
        let samples = decoder.decode(data);
        if !samples.is_empty() {
            on_samples(samples);
        }
    };

    let client = client()?.0;
    let stream = call(client, move |client| {
        block_on(client.create_record_stream(params, callback))
    })
    .map_err(|e| {
        forget_client();
        format!("Failed to open sound server capture: {e}")
    })?;

    let (stop_watchdog, stop_rx) = mpsc::channel::<()>();
    let watched = stream.clone();
    std::thread::Builder::new()
        .name("handy-pulse-watchdog".into())
        .spawn(move || {
            // The client ignores the server's stream-killed notice, so a lost
            // server or source only surfaces as a failed request.
            while let Err(RecvTimeoutError::Timeout) = stop_rx.recv_timeout(WATCHDOG_INTERVAL) {
                if let Err(e) = block_on(watched.timing_info()) {
                    log::warn!("Sound server capture stream failed: {e}");
                    on_failure();
                    return;
                }
            }
        })
        .map_err(|e| format!("Failed to start capture watchdog: {e}"))?;

    Ok(PulseCapture {
        _stream: stream,
        _stop_watchdog: stop_watchdog,
    })
}

/// Returns the shared client and whether it was connected by this call.
fn client() -> Result<(Client, bool), String> {
    let mut guard = CLIENT.lock().unwrap();
    if let Some(client) = guard.as_ref() {
        return Ok((client.clone(), false));
    }

    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(Client::from_env(c"Handy"));
    });
    let client = match rx.recv_timeout(CONNECT_TIMEOUT) {
        Ok(Ok(client)) => client,
        Ok(Err(e)) => return Err(format!("Sound server unavailable: {e}")),
        Err(_) => return Err("Sound server unavailable: connection timed out".to_string()),
    };
    *guard = Some(client.clone());
    Ok((client, true))
}

fn forget_client() {
    *CLIENT.lock().unwrap() = None;
}

/// Runs `op` against the shared client, reconnecting once if a cached
/// connection turns out to be dead (the server restarted since it was used).
fn request<T, F>(op: F) -> Result<T, String>
where
    T: Send + 'static,
    F: Fn(&Client) -> pulseaudio::Result<T> + Clone + Send + 'static,
{
    loop {
        let (client, fresh) = client()?;
        match call(client, op.clone()) {
            Ok(value) => return Ok(value),
            Err(error) => {
                forget_client();
                if fresh || matches!(error, CallError::Timeout) {
                    return Err(error.to_string());
                }
            }
        }
    }
}

/// Bounds a server round trip; a hung server would otherwise block forever.
fn call<T, F>(client: Client, op: F) -> Result<T, CallError>
where
    T: Send + 'static,
    F: FnOnce(&Client) -> pulseaudio::Result<T> + Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(op(&client));
    });
    match rx.recv_timeout(REQUEST_TIMEOUT) {
        Ok(result) => result.map_err(CallError::Client),
        Err(_) => Err(CallError::Timeout),
    }
}

/// Converts server deliveries into whole interleaved f32le frames, carrying a
/// partial frame into the next delivery so channels stay aligned.
struct FrameDecoder {
    frame_bytes: usize,
    partial: Vec<u8>,
    samples: Vec<f32>,
}

impl FrameDecoder {
    fn new(frame_bytes: usize) -> Self {
        Self {
            frame_bytes,
            partial: Vec::with_capacity(frame_bytes),
            samples: Vec::new(),
        }
    }

    fn decode(&mut self, mut data: &[u8]) -> &[f32] {
        self.samples.clear();
        if !self.partial.is_empty() {
            let needed = (self.frame_bytes - self.partial.len()).min(data.len());
            self.partial.extend_from_slice(&data[..needed]);
            data = &data[needed..];
            if self.partial.len() < self.frame_bytes {
                return &self.samples;
            }
            extend_from_f32le(&mut self.samples, &self.partial);
            self.partial.clear();
        }

        let whole = data.len() - data.len() % self.frame_bytes;
        extend_from_f32le(&mut self.samples, &data[..whole]);
        self.partial.extend_from_slice(&data[whole..]);
        &self.samples
    }
}

fn extend_from_f32le(samples: &mut Vec<f32>, bytes: &[u8]) {
    let (frames, _) = bytes.as_chunks::<BYTES_PER_SAMPLE>();
    samples.extend(frames.iter().map(|b| f32::from_le_bytes(*b)));
}

#[cfg(test)]
mod tests {
    use super::{list_sources, FrameDecoder};
    use crate::audio_toolkit::audio::{AudioRecorder, InputDevice};
    use crate::audio_toolkit::VadPolicy;
    use std::time::Duration;

    /// Records two seconds through the full recorder from a live server
    /// source. `HANDY_PULSE_TEST_SOURCE` picks the source by description
    /// substring; the server default is used otherwise.
    #[test]
    #[ignore = "records from a live sound server microphone"]
    fn live_server_capture() {
        let sources = list_sources().expect("sound server should be reachable");
        for source in &sources {
            println!("source: {:?} ({})", source.description, source.name);
        }
        let wanted = std::env::var("HANDY_PULSE_TEST_SOURCE").ok();
        let source = sources
            .into_iter()
            .find(|s| match &wanted {
                Some(w) => s.description.contains(w.as_str()),
                None => s.is_default,
            })
            .expect("requested source should exist");
        println!("capturing from {:?}", source.description);

        let mut recorder = AudioRecorder::new().unwrap();
        recorder.open(Some(InputDevice::Pulse(source))).unwrap();
        let ready = recorder.start(VadPolicy::Disabled).unwrap();
        ready
            .recv_timeout(Duration::from_secs(2))
            .expect("first samples should arrive");
        std::thread::sleep(Duration::from_secs(2));
        let samples = recorder.stop().unwrap();
        assert!(!recorder.needs_reopen());
        recorder.close().unwrap();

        let rms = (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt();
        println!("captured {} samples at 16 kHz, rms {rms:.5}", samples.len());
        assert!(samples.len() > 16_000 * 3 / 2, "expected ~2s of audio");
        assert!(
            samples.iter().any(|&s| s != 0.0),
            "expected non-silent input"
        );
    }

    fn bytes(samples: &[f32]) -> Vec<u8> {
        samples.iter().flat_map(|s| s.to_le_bytes()).collect()
    }

    #[test]
    fn decodes_whole_frames() {
        let mut decoder = FrameDecoder::new(8);
        let data = bytes(&[0.1, 0.2, 0.3, 0.4]);
        assert_eq!(decoder.decode(&data), &[0.1, 0.2, 0.3, 0.4]);
    }

    #[test]
    fn carries_split_frames_without_shifting_channels() {
        let mut decoder = FrameDecoder::new(8);
        let data = bytes(&[0.1, 0.2, 0.3, 0.4, 0.5, 0.6]);

        // First delivery ends three bytes into the second frame's left sample.
        assert_eq!(decoder.decode(&data[..11]), &[0.1, 0.2]);
        // A delivery too short to finish the frame yields nothing.
        assert!(decoder.decode(&data[11..13]).is_empty());
        assert_eq!(decoder.decode(&data[13..]), &[0.3, 0.4, 0.5, 0.6]);
    }
}
