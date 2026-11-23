//! Standalone Rust runner. It has no Synthphone paths or startup expectations.
use super::{fs, PathBuf, Result};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    io::Write,
    time::{SystemTime, UNIX_EPOCH},
};
use virtual_daisy::{board::Board, machine::Machine};

fn help() -> &'static str {
    "Usage: virtual-daisy demo --board rev5 --out results/demo\n       virtual-daisy emulate --elf firmware.elf --board rev4|rev5|rev7 --out results/capture\n         [--duration-ms 1000] [--max-steps 50000000] [--input audio.wav] [--qspi flash.bin]\n\nRust only. No Renode or .NET installation is needed. Captures get a new subdirectory; earlier runs are preserved."
}
pub fn run(args: &[OsString]) -> Result<()> {
    if args.iter().any(|s| s == "--help" || s == "-h") {
        println!("{}", help());
        return Ok(());
    }
    let demo = args[0] == "demo";
    let mut elf = None;
    let mut board = None;
    let mut out = None;
    let mut duration = if demo { 10 } else { 1000 };
    let mut steps = 50_000_000;
    let mut input = None;
    let mut qspi = None;
    let mut i = 1;
    while i < args.len() {
        let option = args[i].to_str().ok_or("option must be UTF-8")?;
        i += 1;
        let value = args
            .get(i)
            .ok_or_else(|| format!("missing value for {option}\n{}", help()))?;
        i += 1;
        match option {
            "--elf" => elf = Some(PathBuf::from(value)),
            "--board" => {
                board = Some(
                    value
                        .to_str()
                        .ok_or("board must be rev4, rev5 or rev7")?
                        .parse::<Board>()?,
                )
            }
            "--out" => out = Some(PathBuf::from(value)),
            "--input" => input = Some(PathBuf::from(value)),
            "--qspi" => qspi = Some(PathBuf::from(value)),
            "--duration-ms" => {
                duration = value
                    .to_str()
                    .ok_or("duration must be an integer")?
                    .parse::<u64>()?
            }
            "--max-steps" => {
                steps = value
                    .to_str()
                    .ok_or("step budget must be an integer")?
                    .parse::<u64>()?
            }
            _ => return Err(format!("unknown option {option}\n{}", help()).into()),
        }
    }
    let board = board.ok_or_else(|| format!("choose --board rev4, rev5 or rev7\n{}", help()))?;
    let out = out.ok_or_else(|| format!("choose an --out directory\n{}", help()))?;
    if !(1..=10_000).contains(&duration) || steps == 0 {
        return Err("duration must be 1..10000 ms and the step budget must be nonzero".into());
    }
    if demo && elf.is_some() {
        return Err("demo supplies its own ARM image; omit --elf".into());
    }
    if demo && input.is_some() {
        return Err("demo checks memory only; use emulate for WAV input".into());
    }
    let image = if demo {
        demo_image()
    } else {
        fs::read(elf.as_ref().ok_or("emulate requires --elf firmware.elf")?)?
    };
    let mut machine = if demo {
        Machine::from_binary(board, &image)?
    } else {
        Machine::from_elf(board, &image)?
    };
    let flash_input = qspi.map(fs::read).transpose()?;
    if let Some(bytes) = &flash_input {
        machine.restore_nonvolatile_image(bytes)?;
    }
    let flash_revision = machine.nonvolatile_revision();
    let mut input_image = None;
    let input_rate = if let Some(path) = input {
        let (rate, samples, bytes) = read_input(&path)?;
        machine.input(&samples)?;
        input_image = Some(bytes);
        Some(rate)
    } else {
        None
    };
    let mut audio = Vec::new();
    let mut serial = Vec::new();
    let mut failure = None;
    let mut rate: Option<virtual_daisy::clock::Rate> = None;
    // Drain output throughout the run so long captures do not retain board FIFOs.
    for end in (1..=duration).map(|ms| ms * 1_000_000) {
        if let Err(error) = machine.run_until(end, steps.saturating_sub(machine.steps())) {
            failure = Some(error.to_string());
            break;
        }
        if let Some(current) = machine.sample_rate() {
            if rate.is_some_and(|previous| {
                u128::from(previous.numerator) * u128::from(current.denominator)
                    != u128::from(current.numerator) * u128::from(previous.denominator)
            }) {
                failure = Some(
                    "sample rate changed during capture; use separate runs for each rate"
                        .to_owned(),
                );
                break;
            }
            rate = Some(current);
        }
        audio.extend(machine.take_audio());
        serial.extend(machine.take_serial());
    }
    audio.extend(machine.take_audio());
    serial.extend(machine.take_serial());
    if failure.is_none() && demo && machine.read32(0x20000000)? != 0xda155eed {
        failure = Some("ARM demo did not write its completion marker".to_owned());
    }
    let rate = rate.or(machine.sample_rate());
    if let (Some(input), Some(output)) = (input_rate, rate) {
        if (f64::from(input) - output.hz()).abs() > 1.0 {
            failure = Some(format!("input is {input} Hz; firmware configured {:.6} Hz. Resample the input to the firmware rate",output.hz()));
        }
    }
    fs::create_dir_all(&out)?;
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    let mut index = 0;
    let directory = loop {
        let p = out.join(format!("{stamp}-{index:02}"));
        match fs::create_dir(&p) {
            Ok(()) => break p,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => index += 1,
            Err(e) => return Err(e.into()),
        }
    };
    fs::write(
        directory.join(if demo { "firmware.bin" } else { "firmware.elf" }),
        &image,
    )?;
    let hash = format!("{:x}", Sha256::digest(&image));
    let rate_text = rate.map_or_else(
        || "disabled".to_owned(),
        |r| format!("{}/{} ({:.6} Hz)", r.numerator, r.denominator, r.hz()),
    );
    let report=format!("backend=rust\nboard={}\ncodec={}\nfirmware_sha256={hash}\nelapsed_ns={}\nscheduler_steps={}\nfinal_pc=0x{:08x}\ncfsr=0x{:08x}\nhfsr=0x{:08x}\naudio_frames={}\nsample_rate={rate_text}\nserial_bytes={}\nhardware_equivalence=UNVERIFIED\n",board.id(),board.codec(),machine.time_ns(),machine.steps(),machine.registers()[15],machine.read32(0xe000ed28)?,machine.read32(0xe000ed2c)?,audio.len(),serial.len());
    let report = format!(
        "sdk_version={}\nstatus={}\naudio_boundary=serial\nsystem_resets={}\nqueued_input_frames={}\n{report}error={}\n",
        env!("CARGO_PKG_VERSION"),
        if failure.is_none() {
            "completed"
        } else {
            "failed"
        },
        machine.resets(),machine.queued_input_frames(),
        failure.as_deref().unwrap_or("none")
    );
    fs::write(directory.join("report.txt"), &report)?;
    if let Some(bytes) = input_image {
        fs::write(directory.join("input.wav"), bytes)?;
    }
    if let Some(bytes) = flash_input {
        fs::write(directory.join("input.vdnor"), bytes)?;
    }
    fs::write(directory.join("memory.txt"), machine.memory_report())?;
    if machine.nonvolatile_revision() != flash_revision {
        fs::write(directory.join("qspi.vdnor"), machine.nonvolatile_image())?;
    }
    let mut bytes = fs::File::create(directory.join("serial.csv"))?;
    writeln!(bytes, "at_ns,byte")?;
    for (time, b) in serial {
        writeln!(bytes, "{time},{b:02x}")?;
    }
    if let Some(rate) = rate.filter(|_| !audio.is_empty()) {
        let mut wav = hound::WavWriter::create(
            directory.join("audio.wav"),
            hound::WavSpec {
                channels: 2,
                sample_rate: rate.hz().round() as u32,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )?;
        for frame in audio {
            for sample in frame {
                wav.write_sample(sample)?;
            }
        }
        wav.finalize()?;
    }
    if demo && failure.is_none() {
        println!("ARM memory check passed.");
    }
    println!("{report}Saved {}", directory.display());
    if let Some(error) = failure {
        return Err(format!("{error}; capture saved at {}", directory.display()).into());
    }
    Ok(())
}
fn demo_image() -> Vec<u8> {
    let mut bytes = vec![0; 0x98];
    bytes[..4].copy_from_slice(&0x20010000u32.to_le_bytes());
    bytes[4..8].copy_from_slice(&0x08000081u32.to_le_bytes());
    bytes[0x80..0x88].copy_from_slice(&[3, 0x48, 4, 0x49, 1, 0x60, 0xfe, 0xe7]);
    bytes[0x90..0x94].copy_from_slice(&0x20000000u32.to_le_bytes());
    bytes[0x94..0x98].copy_from_slice(&0xda155eedu32.to_le_bytes());
    bytes
}
type InputImage = (u32, Vec<[f32; 2]>, Vec<u8>);
fn read_input(path: &std::path::Path) -> Result<InputImage> {
    if fs::metadata(path)?.len() > 16 * 1024 * 1024 {
        return Err("input WAV exceeds 16 MiB".into());
    }
    let bytes = fs::read(path)?;
    let mut reader = hound::WavReader::new(std::io::Cursor::new(&bytes))?;
    let spec = reader.spec();
    if ![1, 2].contains(&spec.channels) || reader.len() > 2_000_000 {
        return Err("input must be a mono/stereo WAV with at most 2 million samples".into());
    }
    let values: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<std::result::Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = 2f32.powi(i32::from(spec.bits_per_sample) - 1);
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / scale))
                .collect::<std::result::Result<_, _>>()?
        }
    };
    if values.iter().any(|s| !s.is_finite() || s.abs() > 1.) {
        return Err("input samples must be finite and within full scale".into());
    }
    if !values.len().is_multiple_of(usize::from(spec.channels)) {
        return Err("input WAV contains an incomplete stereo frame".into());
    }
    Ok((
        spec.sample_rate,
        values
            .chunks_exact(spec.channels as usize)
            .map(|f| [f[0], f[usize::from(spec.channels == 2)]])
            .collect(),
        bytes,
    ))
}
