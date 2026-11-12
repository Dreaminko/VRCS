//! Microphone tap resampling that keeps fractional output phase across input chunks.

pub(crate) const TAP_RATE: u32 = 16_000;

pub(crate) struct TapResampler {
    input_rate: u32,
    source_frames: u64,
    output_frames: u64,
    previous: f32,
}

impl TapResampler {
    pub(crate) fn new(input_rate: u32) -> Self {
        Self {
            input_rate,
            source_frames: 0,
            output_frames: 0,
            previous: 0.0,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.source_frames = 0;
        self.output_frames = 0;
        self.previous = 0.0;
    }

    pub(crate) fn resample(&mut self, samples: &[f32]) -> Vec<f32> {
        let mut output = Vec::with_capacity(
            (samples.len() as u64 * u64::from(TAP_RATE) / u64::from(self.input_rate) + 1) as usize,
        );
        for &sample in samples {
            let frame = self.source_frames;
            while self
                .output_frames
                .saturating_mul(u64::from(self.input_rate))
                <= frame.saturating_mul(u64::from(TAP_RATE))
            {
                let position =
                    self.output_frames as f64 * f64::from(self.input_rate) / f64::from(TAP_RATE);
                let fraction = (position - frame.saturating_sub(1) as f64).clamp(0.0, 1.0) as f32;
                output.push(if frame == 0 {
                    sample
                } else {
                    self.previous + (sample - self.previous) * fraction
                });
                self.output_frames += 1;
            }
            self.source_frames += 1;
            self.previous = sample;
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_phase_across_chunks() {
        let samples = [0.25; 512];
        let mut split = TapResampler::new(44_100);
        let first = split.resample(&samples);
        let second = split.resample(&samples);
        let mut whole = TapResampler::new(44_100);
        let combined = whole.resample(&samples.repeat(2));
        assert_eq!([first, second].concat(), combined);
    }
}
