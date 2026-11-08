//! Audio fixtures and independent measurements used by the regression runner.
const SAMPLE_RATE: f32 = 48_014.312;
const HOP_SIZE: usize = 256;

/// Deterministic voiced fixture; harmonics are (harmonic number, amplitude).
pub fn tone(hz: f64, seconds: f64, harmonics: &[(usize, f64)]) -> Vec<f32> {
    (0..(seconds * SAMPLE_RATE as f64) as usize)
        .map(|i| {
            let phase = std::f64::consts::TAU * hz * i as f64 / SAMPLE_RATE as f64;
            harmonics
                .iter()
                .map(|&(n, a)| a * (phase * n as f64).sin())
                .sum::<f64>() as f32
        })
        .collect()
}

#[derive(Debug)]
pub struct Metrics {
    pub rms: f64,
    pub peak: f32,
    pub dc: f64,
    pub non_finite: usize,
    pub over_full_scale: usize,
    pub max_step: f32,
    /// Silence is measured over whole hops to avoid counting normal zero crossings.
    pub silent_hops: usize,
    /// Smallest / largest hop RMS; useful for sustained-tone dropout detection.
    pub hop_rms_ratio: f64,
}

pub fn metrics(samples: &[f32]) -> Metrics {
    let finite: Vec<f32> = samples.iter().copied().filter(|x| x.is_finite()).collect();
    let n = finite.len().max(1) as f64;
    let hop_rms: Vec<f64> = samples
        .as_chunks::<HOP_SIZE>()
        .0
        .iter()
        .map(|hop| (hop.iter().map(|&x| (x as f64).powi(2)).sum::<f64>() / HOP_SIZE as f64).sqrt())
        .collect();
    let min_rms = hop_rms.iter().copied().fold(f64::INFINITY, f64::min);
    let max_rms = hop_rms.iter().copied().fold(0.0, f64::max);
    Metrics {
        rms: (finite.iter().map(|&x| (x as f64).powi(2)).sum::<f64>() / n).sqrt(),
        peak: finite.iter().map(|x| x.abs()).fold(0.0, f32::max),
        dc: finite.iter().map(|&x| x as f64).sum::<f64>() / n,
        non_finite: samples.len() - finite.len(),
        over_full_scale: finite.iter().filter(|x| x.abs() > 1.0).count(),
        max_step: samples
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f32::max),
        silent_hops: samples
            .as_chunks::<HOP_SIZE>()
            .0
            .iter()
            .filter(|hop| {
                hop.iter().all(|x| x.is_finite())
                    && hop.iter().map(|&x| x as f64 * x as f64).sum::<f64>() / (HOP_SIZE as f64)
                        < 1e-10
            })
            .count(),
        hop_rms_ratio: if max_rms > 0.0 {
            min_rms / max_rms
        } else {
            0.0
        },
    }
}

/// Independent normalized autocorrelation oracle (not the firmware's HPS/YIN).
/// For steady monophonic passages only; a missing estimate fails pitch criteria.
pub fn estimate_pitch(samples: &[f32]) -> Option<f64> {
    if samples.len() < 4096 || metrics(samples).rms < 1e-5 {
        return None;
    }
    let x: Vec<f64> = samples.iter().step_by(4).map(|&x| x as f64).collect();
    let sr = SAMPLE_RATE as f64 / 4.0;
    let min_lag = (sr / 1000.0) as usize;
    let max_lag = (sr / 65.0) as usize;
    let window = x.len() - max_lag - 1;
    let mean = x.iter().sum::<f64>() / x.len() as f64;
    let mut corr = vec![0.0; max_lag + 1];
    for (lag, value) in corr.iter_mut().enumerate().skip(min_lag - 1) {
        let (mut xy, mut xx, mut yy) = (0.0, 0.0, 0.0);
        for i in 0..window {
            let a = x[i] - mean;
            let b = x[i + lag] - mean;
            xy += a * b;
            xx += a * a;
            yy += b * b;
        }
        *value = xy / (xx * yy).sqrt().max(1e-30);
    }
    for lag in min_lag..max_lag {
        if corr[lag] > 0.8 && corr[lag] > corr[lag - 1] && corr[lag] >= corr[lag + 1] {
            let denominator = corr[lag - 1] - 2.0 * corr[lag] + corr[lag + 1];
            let offset = if denominator.abs() > 1e-12 {
                0.5 * (corr[lag - 1] - corr[lag + 1]) / denominator
            } else {
                0.0
            };
            return Some(sr / (lag as f64 + offset));
        }
    }
    None
}

/// Amplitude ratio in decibels.
pub fn db_ratio(a: f64, b: f64) -> f64 {
    20.0 * (a / b).log10()
}
