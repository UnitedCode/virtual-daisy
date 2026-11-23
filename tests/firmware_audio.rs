#![cfg(feature = "rust-host")]
//! Optional consumer test for a supplied Synthphone firmware ELF.
//! The I2C sink records traffic; it does not validate a physical OLED.
use sha2::{Digest, Sha256};
use std::{cell::RefCell, fs, rc::Rc};
use virtual_daisy::{
    audio,
    board::Board,
    machine::{Carrier, Error, Machine},
};

struct DisplaySink(Rc<RefCell<usize>>);
impl Carrier for DisplaySink {
    fn i2c_write(&mut self, bus: u8, address: u8, _bytes: &[u8]) -> Result<(), Error> {
        if (bus, address) != (1, 0x3c) {
            return Err("unexpected external I2C device in Synthphone fixture".into());
        }
        *self.0.borrow_mut() += 1;
        Ok(())
    }
}

#[test]
#[ignore = "requires VIRTUAL_DAISY_FIRMWARE pointing to a Synthphone ELF"]
fn supplied_synthphone_firmware_processes_audio_on_all_seed_revisions() {
    let firmware = fs::read(
        std::env::var_os("VIRTUAL_DAISY_FIRMWARE")
            .expect("set VIRTUAL_DAISY_FIRMWARE to the immutable input ELF"),
    )
    .unwrap();
    let captures = std::env::var_os("VIRTUAL_DAISY_AUDIO_OUT").map(std::path::PathBuf::from);
    let expected_hz = std::env::var("VIRTUAL_DAISY_EXPECTED_AUDIO_HZ")
        .ok()
        .map(|value| {
            let hz: f64 = value
                .parse()
                .expect("VIRTUAL_DAISY_EXPECTED_AUDIO_HZ must be a frequency in Hz");
            assert!(hz.is_finite() && (1.0..=384_000.0).contains(&hz));
            hz
        });
    if let Some(path) = &captures {
        fs::create_dir(path).expect("choose a new audio capture directory");
        fs::write(path.join("firmware.elf"), &firmware).unwrap();
        fs::write(
            path.join("firmware.sha256"),
            format!("{:x}\n", Sha256::digest(&firmware)),
        )
        .unwrap();
        let mut wav = hound::WavWriter::create(
            path.join("input.wav"),
            hound::WavSpec {
                channels: 2,
                sample_rate: 48_014,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )
        .unwrap();
        for s in audio::tone(220., 3., &[(1, 0.15)]) {
            wav.write_sample(s).unwrap();
            wav.write_sample(s).unwrap();
        }
        wav.finalize().unwrap();
    }
    let boards = std::env::var("VIRTUAL_DAISY_AUDIO_BOARD")
        .ok()
        .map(|s| {
            vec![s
                .parse::<Board>()
                .expect("audio board must be rev4, rev5 or rev7")]
        })
        .unwrap_or_else(|| Board::ALL.to_vec());
    for board in boards {
        let mut m = Machine::from_elf(board, &firmware).unwrap();
        let packets = Rc::default();
        m.attach(Box::new(DisplaySink(Rc::clone(&packets))));
        // Match the reference recording's source clock and interpolation onto
        // the derived SAI clock. No host microphone or audio output is opened.
        let input: Vec<_> = audio::tone(220., 3., &[(1, 0.15)])
            .into_iter()
            .map(|s| [s, s])
            .collect();
        m.input_at_rate(&input, 48_014).unwrap();
        let mut output = Vec::new();
        for step in 1..=20 {
            m.run_until(step * 10_000_000, 6_000_000)
                .unwrap_or_else(|e| panic!("{board:?} at step {step}: {e}"));
            output.extend(m.take_audio());
            if step == 10 {
                eprintln!("{board:?}: first_100ms_frames={}", output.len());
            }
        }
        assert!(*packets.borrow() > 10, "firmware configured its display");
        let rate = m.sample_rate().expect("firmware started its audio clock");
        assert!((47_000.0..49_000.0).contains(&rate.hz()));
        assert!(output.len() > 7_000, "sustained audio clock");
        assert_eq!(m.read32(0xe000ed28).unwrap(), 0, "CFSR");
        assert_eq!(m.read32(0xe000ed2c).unwrap(), 0, "HFSR");
        let sai_status = [m.read32(0x40015818).unwrap(), m.read32(0x40015838).unwrap()];
        let mut rcc = String::new();
        for (name, offset) in [
            ("cr", 0),
            ("pllckselr", 0x28),
            ("pllcfgr", 0x2c),
            ("pll3divr", 0x40),
            ("pll3fracr", 0x44),
        ] {
            use std::fmt::Write;
            writeln!(
                rcc,
                "rcc_{name}={:#010x}",
                m.read32(0x58024400 + offset).unwrap()
            )
            .unwrap();
        }
        eprintln!("{board:?}: SAI status={sai_status:x?}");
        if let Some(path) = &captures {
            let name = format!("{board:?}").to_lowercase();
            let mut wav = hound::WavWriter::create(
                path.join(format!("{name}.wav")),
                hound::WavSpec {
                    channels: 2,
                    sample_rate: rate.hz().round() as u32,
                    bits_per_sample: 32,
                    sample_format: hound::SampleFormat::Float,
                },
            )
            .unwrap();
            for frame in &output {
                for sample in frame {
                    wav.write_sample(*sample).unwrap();
                }
            }
            wav.finalize().unwrap();
            fs::write(
                path.join(format!("{name}.txt")),
                format!(
                    "frames={}\nrate={rate:?}\nsai_status={sai_status:x?}\n{rcc}{}",
                    output.len(),
                    m.memory_report()
                ),
            )
            .unwrap();
        }
        if let Some(hz) = expected_hz {
            let actual = rate.hz();
            let error_ppm = ((actual - hz) / hz).abs() * 1e6;
            assert!(error_ppm<=1.0,"{board:?}: sample rate {actual} Hz differs from expected {hz} Hz by {error_ppm} ppm");
        }
        assert_eq!(
            (sai_status[0] | sai_status[1]) & 1,
            0,
            "no SAI overrun/underrun"
        );
        let tail = &output[output.len() - 4_096..];
        for channel in 0..2 {
            let samples: Vec<_> = tail.iter().map(|s| s[channel]).collect();
            let metrics = audio::metrics(&samples);
            eprintln!(
                "{board:?}: frames={} rate={rate:?} channel={channel} {metrics:?}",
                output.len()
            );
            assert_eq!(metrics.non_finite, 0);
            assert_eq!(metrics.over_full_scale, 0);
            assert!(metrics.rms > 0.005, "firmware did not process its input");
            assert_eq!(
                metrics.silent_hops, 0,
                "sustained tone has no silent blocks"
            );
        }
    }
}
