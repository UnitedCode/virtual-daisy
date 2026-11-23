//! Save recorded PCM without assigning a fictitious clock to untouched audio.
use crate::sai::Sai;
use std::{error::Error, fs, path::Path};

pub(crate) fn save(audio: &Sai, path: &str, sample_rate: u32) -> Result<(), Box<dyn Error>> {
    if sample_rate == 0 {
        if audio.frames != 0 || !audio.output.is_empty() {
            return Err("captured audio has no sample rate".into());
        }
        if Path::new(path).exists() {
            return Err("cannot replace an existing recording with unstarted audio".into());
        }
        fs::write(
            format!("{path}.stats"),
            format!(
                "frames=0\nunderruns={}\noverruns={}\ncapture_state=NO_AUDIO_FRAMES\n",
                audio.underruns, audio.overruns
            ),
        )?;
        fs::write(
            format!("{path}.clock.txt"),
            "capture_state=NO_AUDIO_FRAMES\n",
        )?;
        return Ok(());
    }
    if !(1..=384_000).contains(&sample_rate) {
        return Err("capture rate outside supported range".into());
    }
    let mut wav = hound::WavWriter::create(
        path,
        hound::WavSpec {
            channels: 2,
            sample_rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )?;
    for frame in &audio.output {
        for sample in frame {
            wav.write_sample(*sample)?;
        }
    }
    wav.finalize()?;
    fs::write(
        format!("{path}.stats"),
        format!(
            "frames={}\nunderruns={}\noverruns={}\n",
            audio.frames, audio.underruns, audio.overruns
        ),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Output {
        directory: std::path::PathBuf,
        path: String,
    }
    impl Output {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let directory =
                std::env::temp_dir().join(format!("daisy-audio-save-{}-{id}", std::process::id()));
            fs::create_dir(&directory).unwrap();
            let path = directory.join("capture.wav").to_str().unwrap().to_owned();
            Self { directory, path }
        }
    }
    impl Drop for Output {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.directory).unwrap();
        }
    }
    #[test]
    fn firmware_without_audio_keeps_status_without_a_fake_wav() {
        let output = Output::new();
        save(&Sai::default(), &output.path, 0).unwrap();
        assert!(!Path::new(&output.path).exists());
        assert!(fs::read_to_string(format!("{}.stats", output.path))
            .unwrap()
            .contains("capture_state=NO_AUDIO_FRAMES"));
        assert_eq!(
            fs::read_to_string(format!("{}.clock.txt", output.path)).unwrap(),
            "capture_state=NO_AUDIO_FRAMES\n"
        );
    }
    #[test]
    fn missing_clock_cannot_hide_recorded_samples_or_replace_a_recording() {
        let output = Output::new();
        let mut audio = Sai::default();
        audio.output.push([0.1, -0.2]);
        audio.frames = 1;
        assert!(save(&audio, &output.path, 0).is_err());
        assert!(!Path::new(&output.path).exists());
        fs::write(&output.path, b"previous recording").unwrap();
        assert!(save(&Sai::default(), &output.path, 0).is_err());
        assert_eq!(fs::read(&output.path).unwrap(), b"previous recording");
    }
    #[test]
    fn known_clock_preserves_stereo_samples_after_the_peripheral_stops() {
        let output = Output::new();
        let mut audio = Sai::default();
        audio.sample_rate = 48_187.487_053_149;
        audio.output = vec![[0.25, -0.5], [0.125, -0.75]];
        audio.frames = 2;
        // Disabled serial registers do not replace the rate of recorded PCM.
        save(&audio, &output.path, audio.sample_rate.round() as u32).unwrap();
        let wav = hound::WavReader::open(&output.path).unwrap();
        assert_eq!(wav.spec().sample_rate, 48_187);
        assert_eq!(
            wav.into_samples::<f32>()
                .collect::<Result<Vec<_>, _>>()
                .unwrap(),
            vec![0.25, -0.5, 0.125, -0.75]
        );
    }
    #[test]
    fn explicit_invalid_rates_fail_without_creating_audio() {
        let output = Output::new();
        assert!(save(&Sai::default(), &output.path, 384_001).is_err());
        assert!(!Path::new(&output.path).exists());
    }
    #[test]
    fn unstarted_audio_preserves_fifo_error_counts() {
        let output = Output::new();
        let mut audio = Sai::default();
        audio.underruns = 2;
        audio.overruns = 3;
        save(&audio, &output.path, 0).unwrap();
        let stats = fs::read_to_string(format!("{}.stats", output.path)).unwrap();
        assert!(stats.contains("underruns=2\noverruns=3\n"));
    }
}
