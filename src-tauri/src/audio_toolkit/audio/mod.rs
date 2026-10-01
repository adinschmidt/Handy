// Re-export all audio components
mod device;
#[cfg(target_os = "linux")]
mod pulse;
mod recorder;
mod resampler;
mod split;
mod utils;
mod visualizer;

pub use device::{
    list_input_devices, list_output_devices, CpalDeviceInfo, InputDevice, InputDeviceInfo,
};
pub use recorder::{
    is_microphone_access_denied, is_no_input_device_error, AudioRecorder, VadPolicy,
};
pub use resampler::FrameResampler;
pub use split::split_at_pauses;
pub use utils::{encode_wav_bytes, read_wav_samples, save_wav_file, verify_wav_file};
pub use visualizer::AudioVisualiser;
