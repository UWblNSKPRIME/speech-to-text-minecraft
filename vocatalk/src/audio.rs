//! Audio helpers for the live pipeline: channel downmix and resampling to
//! the engine's 16 kHz mono format.

use anyhow::Result;
use gigastt_core::inference::audio::{resample, SampleRate};

/// Sample rate the GigaAM engine and Silero VAD operate at.
pub const ENGINE_RATE: u32 = 16000;

/// A chunk of interleaved f32 audio in its native device format.
pub struct RawChunk {
    pub samples: Vec<f32>,
    pub channels: usize,
    pub rate: u32,
}

/// Average interleaved channels down to mono.
pub fn downmix_to_mono(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    let frames = interleaved.len() / channels;
    let mut mono = Vec::with_capacity(frames);
    for frame in interleaved.chunks_exact(channels) {
        mono.push(frame.iter().sum::<f32>() / channels as f32);
    }
    mono
}

/// Resample mono audio from its native rate to the engine rate (16 kHz).
pub fn resample_to_engine(samples: &[f32], from_rate: u32) -> Result<Vec<f32>> {
    resample(samples, SampleRate(from_rate), SampleRate(ENGINE_RATE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downmix_averages_channels() {
        let stereo = [1.0, -1.0, 0.5, 0.5, -0.5, 0.5];
        assert_eq!(downmix_to_mono(&stereo, 2), vec![0.0, 0.5, 0.0]);
    }

    #[test]
    fn downmix_mono_is_identity() {
        assert_eq!(downmix_to_mono(&[0.1, 0.2], 1), vec![0.1, 0.2]);
    }

    #[test]
    fn resample_48k_sine_keeps_length_and_frequency() {
        // 0.5 s of a 1 kHz sine at 48 kHz must become ~0.5 s at 16 kHz
        // with the same perceived frequency.
        let input: Vec<f32> = (0..24_000)
            .map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / 48_000.0).sin())
            .collect();
        let output = resample_to_engine(&input, 48_000).unwrap();
        assert!(
            (output.len() as i64 - 8_000).unsigned_abs() <= 32,
            "expected ~8000 samples, got {}",
            output.len()
        );
        let crossings = output
            .windows(2)
            .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
            .count();
        // 1 kHz over 0.5 s is 500 cycles, so ~500 positive-going crossings.
        assert!(
            (crossings as i64 - 500).unsigned_abs() <= 30,
            "expected ~500 positive zero crossings (1 kHz), got {crossings}"
        );
    }

    #[test]
    fn resample_identity_at_engine_rate() {
        let input = vec![0.5; 1600];
        assert_eq!(resample_to_engine(&input, 16_000).unwrap(), input);
    }
}
