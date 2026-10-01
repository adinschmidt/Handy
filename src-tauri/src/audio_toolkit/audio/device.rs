use cpal::traits::{DeviceTrait, HostTrait};

#[cfg(target_os = "linux")]
use super::pulse::{self, PulseSink, PulseSource};

/// A microphone the recorder can open.
#[derive(Clone)]
pub enum InputDevice {
    Cpal(cpal::Device),
    /// A source captured through the PulseAudio/PipeWire sound server.
    #[cfg(target_os = "linux")]
    Pulse(PulseSource),
}

pub struct InputDeviceInfo {
    pub index: String,
    pub name: String,
    pub is_default: bool,
    pub device: InputDevice,
}

/// Lists selectable microphones. On Linux with a sound server these are its
/// sources, since opening a card through cpal's ALSA host would take it away
/// from the server. A failed listing is then an error rather than a fallback
/// to ALSA cards, so a slow server cannot make a selected source look
/// unplugged and reset the user's choice.
pub fn list_input_devices() -> Result<Vec<InputDeviceInfo>, Box<dyn std::error::Error>> {
    #[cfg(target_os = "linux")]
    if pulse::server_expected() {
        return Ok(pulse::list_sources()?
            .into_iter()
            .enumerate()
            .map(|(index, source)| InputDeviceInfo {
                index: index.to_string(),
                name: source.description.clone(),
                is_default: source.is_default,
                device: InputDevice::Pulse(source),
            })
            .collect());
    }

    let host = crate::audio_toolkit::get_cpal_host();
    let default_name = host.default_input_device().and_then(|d| d.name().ok());

    let mut out = Vec::<InputDeviceInfo>::new();

    for (index, device) in host.input_devices()?.enumerate() {
        let name = device.name().unwrap_or_else(|_| "Unknown".into());

        let is_default = Some(name.clone()) == default_name;

        out.push(InputDeviceInfo {
            index: index.to_string(),
            name,
            is_default,
            device: InputDevice::Cpal(device),
        });
    }

    Ok(out)
}

/// An output device sounds can be played on.
#[derive(Clone)]
pub enum OutputDevice {
    Cpal(cpal::Device),
    /// A sink played to through the PulseAudio/PipeWire sound server.
    #[cfg(target_os = "linux")]
    Pulse(PulseSink),
}

pub struct OutputDeviceInfo {
    pub index: String,
    pub name: String,
    pub is_default: bool,
    pub device: OutputDevice,
}

/// Lists selectable output devices, preferring the sound server's sinks on
/// Linux for the same reasons as [`list_input_devices`].
pub fn list_output_devices() -> Result<Vec<OutputDeviceInfo>, Box<dyn std::error::Error>> {
    #[cfg(target_os = "linux")]
    if pulse::server_expected() {
        return Ok(pulse::list_sinks()?
            .into_iter()
            .enumerate()
            .map(|(index, sink)| OutputDeviceInfo {
                index: index.to_string(),
                name: sink.description.clone(),
                is_default: sink.is_default,
                device: OutputDevice::Pulse(sink),
            })
            .collect());
    }

    let host = crate::audio_toolkit::get_cpal_host();
    let default_name = host.default_output_device().and_then(|d| d.name().ok());

    let mut out = Vec::<OutputDeviceInfo>::new();

    for (index, device) in host.output_devices()?.enumerate() {
        let name = device.name().unwrap_or_else(|_| "Unknown".into());

        let is_default = Some(name.clone()) == default_name;

        out.push(OutputDeviceInfo {
            index: index.to_string(),
            name,
            is_default,
            device: OutputDevice::Cpal(device),
        });
    }

    Ok(out)
}
