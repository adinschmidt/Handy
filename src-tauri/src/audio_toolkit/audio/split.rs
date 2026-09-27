use std::ops::Range;

/// Energy frame used to look for pauses: 30 ms at 16 kHz.
const FRAME_SAMPLES: usize = 480;

/// Split audio into contiguous ranges of at most `max_samples`, cutting in the
/// quietest frame near evenly spaced split points so cuts land in pauses
/// rather than mid-word. Audio that already fits comes back as one range.
///
/// Aiming for even pieces keeps every piece at least a third of
/// `max_samples`, so a recording just over the limit never leaves a
/// near-silent sliver that a model might hallucinate on.
pub fn split_at_pauses(samples: &[f32], max_samples: usize) -> Vec<Range<usize>> {
    let search = max_samples / 6;
    let energy = |frame: usize| -> f32 {
        samples[frame..frame + FRAME_SAMPLES]
            .iter()
            .map(|s| s * s)
            .sum()
    };

    let mut ranges = Vec::new();
    let mut start = 0;
    while samples.len() - start > max_samples {
        let remaining = samples.len() - start;
        let target = start + remaining / remaining.div_ceil(max_samples);
        let search_end = (target + search).min(start + max_samples);
        let cut = (target - search..search_end.saturating_sub(FRAME_SAMPLES))
            .step_by(FRAME_SAMPLES)
            .min_by(|&a, &b| energy(a).total_cmp(&energy(b)))
            .map_or(target, |frame| frame + FRAME_SAMPLES / 2);
        ranges.push(start..cut);
        start = cut;
    }
    ranges.push(start..samples.len());
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: usize = 16_000;
    const MAX: usize = 30 * RATE;

    /// Constant-level stand-in for speech, with the given sample ranges silent.
    fn speech_with_pauses(len: usize, pauses: &[Range<usize>]) -> Vec<f32> {
        let mut samples = vec![0.5; len];
        for pause in pauses {
            samples[pause.clone()].fill(0.0);
        }
        samples
    }

    #[test]
    fn short_audio_is_a_single_range() {
        let samples = speech_with_pauses(MAX, &[]);
        assert_eq!(split_at_pauses(&samples, MAX), vec![0..MAX]);
    }

    #[test]
    fn cuts_inside_the_pause_near_the_split_point() {
        let pause = 22 * RATE..22 * RATE + RATE * 4 / 10;
        let samples = speech_with_pauses(50 * RATE, &[pause.clone()]);

        let ranges = split_at_pauses(&samples, MAX);

        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0].start, 0);
        assert_eq!(ranges[0].end, ranges[1].start);
        assert_eq!(ranges[1].end, samples.len());
        assert!(pause.contains(&ranges[0].end), "cut at {}", ranges[0].end);
    }

    #[test]
    fn trailing_silence_does_not_become_its_own_piece() {
        // A recording just over the limit whose quietest audio is the tail.
        let len = MAX + RATE / 2;
        let samples = speech_with_pauses(len, &[MAX..len]);

        let ranges = split_at_pauses(&samples, MAX);

        assert_eq!(ranges.len(), 2);
        assert!(ranges.iter().all(|r| r.len() >= MAX / 3 && r.len() <= MAX));
    }
}
