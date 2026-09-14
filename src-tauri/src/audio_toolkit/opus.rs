use anyhow::{ensure, Context, Result};
use ogg::{PacketWriteEndInfo, PacketWriter};
use opus::{Application, Bitrate, Channels, Encoder};

const SAMPLE_RATE: u32 = 16_000;
const FRAME_SAMPLES: usize = 320;
// Ogg Opus counts time at 48 kHz, regardless of the encoder's input rate.
const GRANULE_SCALE: u64 = 3;

/// Encode Handy's 16 kHz mono samples as Ogg Opus at a target 32 kbps.
pub fn encode_ogg_opus(samples: &[f32]) -> Result<Vec<u8>> {
    ensure!(!samples.is_empty(), "Recording has no audio samples");
    let mut encoder = Encoder::new(SAMPLE_RATE, Channels::Mono, Application::Audio)
        .context("Failed to create Opus encoder")?;
    encoder.set_bitrate(Bitrate::Bits(32_000))?;
    let lookahead = usize::try_from(encoder.get_lookahead()?)?;
    let pre_skip = u16::try_from(lookahead as u64 * GRANULE_SCALE)?;

    let mut writer = PacketWriter::new(Vec::new());
    let mut header = b"OpusHead".to_vec();
    header.extend_from_slice(&[1, 1]); // Version 1, mono.
    header.extend_from_slice(&pre_skip.to_le_bytes());
    header.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    header.extend_from_slice(&[0, 0, 0]); // Zero output gain, channel mapping family 0.
    writer.write_packet(header, 1, PacketWriteEndInfo::EndPage, 0)?;

    let mut tags = b"OpusTags".to_vec();
    tags.extend_from_slice(&5u32.to_le_bytes());
    tags.extend_from_slice(b"Handy");
    tags.extend_from_slice(&0u32.to_le_bytes());
    writer.write_packet(tags, 1, PacketWriteEndInfo::EndPage, 0)?;

    // Flush the codec delay, then trim padding with the final granule position.
    let frame_count = (samples.len() + lookahead).div_ceil(FRAME_SAMPLES);
    let final_granule = samples.len() as u64 * GRANULE_SCALE + u64::from(pre_skip);
    let mut frame = [0.0; FRAME_SAMPLES];
    let mut packet = [0; 1275];
    for index in 0..frame_count {
        frame.fill(0.0);
        let start = index * FRAME_SAMPLES;
        if start < samples.len() {
            let end = (start + FRAME_SAMPLES).min(samples.len());
            for (output, input) in frame.iter_mut().zip(&samples[start..end]) {
                *output = input.clamp(-1.0, 1.0);
            }
        }
        let len = encoder.encode_float(&frame, &mut packet)?;
        let last = index + 1 == frame_count;
        let end = if last {
            PacketWriteEndInfo::EndStream
        } else if (index + 1) % 50 == 0 {
            PacketWriteEndInfo::EndPage
        } else {
            PacketWriteEndInfo::NormalPacket
        };
        let granule = if last {
            final_granule
        } else {
            (index + 1) as u64 * FRAME_SAMPLES as u64 * GRANULE_SCALE
        };
        writer.write_packet(packet[..len].to_vec(), 1, end, granule)?;
    }
    Ok(writer.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ogg::PacketReader;
    use opus::Decoder;
    use std::io::Cursor;

    #[test]
    fn round_trip_preserves_duration_and_final_audio() {
        for sample_count in [1, FRAME_SAMPLES, 16_337] {
            let samples: Vec<f32> = (0..sample_count)
                .map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / 16_000.0).sin() * 0.5)
                .collect();
            let bytes = encode_ogg_opus(&samples).unwrap();
            if sample_count > 16_000 {
                assert!(bytes.len() < samples.len() * 2 / 4);
            }
            let mut reader = PacketReader::new(Cursor::new(bytes));
            let header = reader.read_packet_expected().unwrap();
            assert_eq!(&header.data[..8], b"OpusHead");
            let skip = u16::from_le_bytes([header.data[10], header.data[11]]) as usize;
            assert_eq!(
                &reader.read_packet_expected().unwrap().data[..8],
                b"OpusTags"
            );
            let mut decoder = Decoder::new(16_000, Channels::Mono).unwrap();
            let mut decoded = Vec::new();
            let mut frame = [0.0; FRAME_SAMPLES];
            let mut end = 0;
            let mut ended = false;
            while let Some(packet) = reader.read_packet().unwrap() {
                let count = decoder
                    .decode_float(&packet.data, &mut frame, false)
                    .unwrap();
                decoded.extend_from_slice(&frame[..count]);
                if packet.last_in_stream() {
                    end = packet.absgp_page() as usize;
                    ended = true;
                }
            }
            assert!(ended);
            assert_eq!(end - skip, sample_count * 3);
            let trimmed = &decoded[skip / 3..end / 3];
            assert_eq!(trimmed.len(), samples.len());
            if sample_count > FRAME_SAMPLES {
                let error = trimmed
                    .iter()
                    .zip(&samples)
                    .map(|(a, b)| (a - b).powi(2))
                    .sum::<f32>()
                    / sample_count as f32;
                assert!(error < 0.02, "Audio alignment or quality changed: {error}");
                assert!(trimmed[trimmed.len() - 80..].iter().any(|s| s.abs() > 0.1));
            }
        }
    }
}
