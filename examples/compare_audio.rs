//! Compare serial PCM captures without hiding different startup frame counts.
use std::{error::Error, path::Path};
fn read(path: &Path) -> Result<(hound::WavSpec, Vec<f32>), Box<dyn Error>> {
    let wav = hound::WavReader::open(path)?;
    let spec = wav.spec();
    if spec.channels != 2
        || spec.sample_format != hound::SampleFormat::Float
        || spec.bits_per_sample != 32
    {
        return Err("comparison expects stereo float32 WAV captures".into());
    }
    let samples = wav.into_samples::<f32>().collect::<Result<Vec<_>, _>>()?;
    if !samples.len().is_multiple_of(2) || samples.iter().any(|s| !s.is_finite()) {
        return Err("capture has an incomplete frame or non-finite sample".into());
    }
    Ok((spec, samples))
}
fn main() -> Result<(), Box<dyn Error>> {
    let mut args: Vec<_> = std::env::args_os().skip(1).collect();
    let exact = args
        .last()
        .is_some_and(|arg| arg == "--require-exact-overlap");
    if exact {
        args.pop();
    }
    if !(2..=3).contains(&args.len()) || (exact && args.len() != 2) {
        return Err("Usage: cargo run --release --example compare_audio -- LEFT.wav RIGHT.wav [SKIP_FRAMES | --require-exact-overlap]".into());
    }
    let skip: usize = args
        .get(2)
        .map(|s| s.to_str().ok_or("skip must be an integer"))
        .transpose()?
        .map(str::parse)
        .transpose()?
        .unwrap_or(0);
    let (a, left) = read(Path::new(&args[0]))?;
    let (b, right) = read(Path::new(&args[1]))?;
    let first_signal = |samples: &[f32]| {
        samples
            .as_chunks::<2>()
            .0
            .iter()
            .position(|frame| frame.iter().any(|s| s.abs() > 1e-7))
    };
    if a.sample_rate != b.sample_rate {
        return Err("capture sample rates differ".into());
    }
    let start = skip.checked_mul(2).ok_or("skip overflow")?;
    let end = left.len().min(right.len());
    if start >= end {
        return Err("no overlapping samples after the skipped frames".into());
    }
    let mut identical = 0;
    let mut peak = 0.0_f64;
    let mut squared = 0.0;
    for (&x, &y) in left[start..end].iter().zip(&right[start..end]) {
        identical += usize::from(x.to_bits() == y.to_bits());
        let error = f64::from(x) - f64::from(y);
        peak = peak.max(error.abs());
        squared += error * error;
    }
    println!("left_frames={}\nright_frames={}\nsample_rate={}\nskipped_frames={skip}\ncompared_frames={}\nidentical_samples={identical}\ncompared_samples={}\npeak_absolute_error={peak:.9}\nrms_error={:.9}\nalignment=serial_frame_index\ntiming_equivalence=UNVERIFIED", left.len()/2, right.len()/2, a.sample_rate, (end-start)/2, end-start, (squared/(end-start) as f64).sqrt());
    println!(
        "left_first_signal_frame={:?}\nright_first_signal_frame={:?}",
        first_signal(&left),
        first_signal(&right)
    );
    // A diagnostic search, reported separately from the unshifted comparison.
    // A better match after shifting is evidence of a latency difference.
    let frames = (end - start) / 2;
    let radius = (frames / 4).min(2048);
    let window = (frames - 2 * radius).min(4096);
    if window >= 1024 {
        let mut best = (f64::INFINITY, 0isize);
        for offset in -(radius as isize)..=radius as isize {
            let l = start / 2 + radius + (-offset).max(0) as usize;
            let r = start / 2 + radius + offset.max(0) as usize;
            let error = (0..window)
                .map(|i| {
                    let difference = f64::from(left[2 * (l + i)]) - f64::from(right[2 * (r + i)]);
                    difference * difference
                })
                .sum::<f64>()
                / window as f64;
            if error < best.0 || error == best.0 && offset.abs() < best.1.abs() {
                best = (error, offset);
            }
        }
        println!("diagnostic_window_frames={window}\nbest_right_offset_frames={}\noffset_channel0_rms_error={:.9}\noffset_scope=constant_delay_diagnostic_only", best.1, best.0.sqrt());
    }
    if exact && identical != end - start {
        return Err(format!(
            "PCM differs in {} of {} overlapping channel samples",
            end - start - identical,
            end - start
        )
        .into());
    }
    Ok(())
}
