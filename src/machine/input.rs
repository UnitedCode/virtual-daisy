//! Bounded serial frames, optionally timestamped at a separate source clock.
use crate::clock::Rate;
use std::collections::VecDeque;
#[derive(Default)]
pub(super) struct Input {
    frames: VecDeque<[f32; 2]>,
    source_rate: Option<u32>,
    position: f64,
    base: u64,
}
impl Input {
    pub fn len(&self) -> usize {
        self.frames.len()
    }
    pub fn queue(&mut self, samples: &[[f32; 2]], rate: Option<u32>) -> Result<(), &'static str> {
        if rate.is_some_and(|r| r == 0 || r > 384_000) {
            return Err("input source rate must be 1..384000 Hz");
        }
        if rate != self.source_rate && (self.position != 0.0 || !self.frames.is_empty()) {
            return Err("cannot mix serial/timestamped input or change its source rate");
        }
        if samples
            .iter()
            .flatten()
            .any(|s| !s.is_finite() || s.abs() > 1.)
        {
            return Err("input samples must be finite and within full scale");
        }
        if self.frames.len().saturating_add(samples.len()) > 1_000_000 {
            return Err(
                "serial input queue exceeds one million frames; feed it in smaller batches",
            );
        }
        self.source_rate = rate;
        self.frames.extend(samples.iter().copied());
        Ok(())
    }
    pub fn next(&mut self, clock: Rate) -> [f32; 2] {
        if let Some(rate) = self.source_rate {
            let first = self.frames.front().copied().unwrap_or([0.; 2]);
            let next = self.frames.get(1).copied().unwrap_or(first);
            let fraction = self.position.fract() as f32;
            let sample = std::array::from_fn(|c| first[c] + (next[c] - first[c]) * fraction);
            self.position += f64::from(rate) / clock.hz();
            let end = self.position as u64;
            for _ in self.base..end {
                self.frames.pop_front();
            }
            self.base = end;
            sample
        } else {
            self.position += 1.;
            self.frames.pop_front().unwrap_or([0.; 2])
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timestamped_samples_match_the_reference_interpolator_on_fractional_clocks() {
        let samples: Vec<_> = (0..2000)
            .map(|n| [(n as f32 * 0.17).sin(), (n as f32 * 0.13).cos()])
            .collect();
        for rate in [8_000, 44_100, 48_014, 96_000] {
            let clock = Rate {
                numerator: 3_256_703_125,
                denominator: 67_584,
            };
            let mut input = Input::default();
            input.queue(&samples, Some(rate)).unwrap();
            let mut position = 0.0_f64;
            for _ in 0..3000 {
                let first = samples.get(position as usize).copied().unwrap_or([0.; 2]);
                let next = samples.get(position as usize + 1).copied().unwrap_or(first);
                let expected: [f32; 2] = std::array::from_fn(|c| {
                    first[c] + (next[c] - first[c]) * position.fract() as f32
                });
                assert_eq!(input.next(clock), expected);
                position += f64::from(rate) / clock.hz();
            }
        }
    }
    #[test]
    fn chunked_refills_keep_the_same_phase_as_a_single_recording() {
        let samples: Vec<_> = (0..1024)
            .map(|n| [n as f32 / 1024., -(n as f32) / 1024.])
            .collect();
        let clock = Rate {
            numerator: 48_000,
            denominator: 1,
        };
        let mut whole = Input::default();
        whole.queue(&samples, Some(44_100)).unwrap();
        let mut parts = Input::default();
        parts.queue(&samples[..512], Some(44_100)).unwrap();
        for n in 0..1000 {
            if n == 400 {
                parts.queue(&samples[512..], Some(44_100)).unwrap();
            }
            assert_eq!(whole.next(clock), parts.next(clock));
        }
    }
    #[test]
    fn serial_frames_are_not_resampled_and_invalid_updates_are_transactional() {
        let clock = Rate {
            numerator: 96_000,
            denominator: 1,
        };
        let mut input = Input::default();
        input.queue(&[[0.5, -0.25], [1., -1.]], None).unwrap();
        assert!(input.queue(&[[f32::NAN, 0.]], None).is_err());
        assert!(input.queue(&[], Some(48_000)).is_err());
        assert_eq!(input.len(), 2);
        assert_eq!(input.next(clock), [0.5, -0.25]);
        assert_eq!(input.next(clock), [1., -1.]);
        assert_eq!(input.next(clock), [0., 0.]);
    }
}
