//! Stateful browser/codec clock conversion, independent of firmware DSP.
#[derive(Default)]
pub struct Resampler {
    phase: f64,
    last: f32,
}
impl Resampler {
    pub fn process(&mut self, input: &[f32], source: f64, target: f64) -> Vec<f32> {
        let ratio = source / target;
        if !ratio.is_finite() || ratio <= 0. {
            return Vec::new();
        }
        let mut output = Vec::with_capacity((input.len() as f64 / ratio).ceil() as usize + 2);
        for &sample in input {
            let sample = if sample.is_finite() { sample } else { 0. };
            while self.phase < 1. {
                output.push(self.last + (sample - self.last) * self.phase as f32);
                self.phase += ratio;
            }
            self.phase -= 1.;
            self.last = sample;
        }
        output
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn splitting_capture_packets_does_not_change_the_waveform() {
        let input = (0..44100)
            .map(|n| (n as f64 * 220. * std::f64::consts::TAU / 44100.).sin() as f32 * 0.16)
            .collect::<Vec<_>>();
        let whole = Resampler::default().process(&input, 44100., 48187.487053);
        let mut resampler = Resampler::default();
        let split = input
            .chunks(137)
            .flat_map(|chunk| resampler.process(chunk, 44100., 48187.487053))
            .collect::<Vec<_>>();
        assert_eq!(whole, split);
        assert!((whole.len() as f64 - 48187.487053).abs() <= 1.);
        let error = whole
            .iter()
            .enumerate()
            .map(|(n, &sample)| {
                let time = (n as f64 / 48187.487053 - 1. / 44100.).max(0.);
                let expected = (time * 220. * std::f64::consts::TAU).sin() * 0.16;
                (f64::from(sample) - expected).powi(2)
            })
            .sum::<f64>()
            / whole.len() as f64;
        assert!(
            error.sqrt() < 0.00002,
            "resampling changed the tone: {error}"
        );
    }
    #[test]
    fn codec_output_follows_the_browser_clock_without_drift() {
        let mut resampler = Resampler::default();
        let input = vec![0.25; 48187];
        let mut frames = 0;
        for _ in 0..10 {
            frames += resampler.process(&input, 48187.487053, 48000.).len();
        }
        assert!((frames as i64 - 480000).abs() <= 6);
    }
}
