//! Exercise the unchanged firmware via its codec and baud-paced MIDI input.
use std::{fs, time::Instant};
use virtual_daisy_web::Firmware;
fn main() {
    let path = std::env::args().nth(1).expect("firmware .bin");
    let kind = std::env::args().nth(2).unwrap_or_else(|| "mic".into());
    let mut f = Firmware::new(&fs::read(path).unwrap()).unwrap();
    let input: Vec<f32> = (0..48_000)
        .map(|n| {
            if kind == "mic" {
                (n as f32 * 220.0 * std::f32::consts::TAU / 48_014.312).sin() * 0.2
            } else {
                0.0
            }
        })
        .collect();
    f.input(&input);
    let start = Instant::now();
    let mut output = Vec::new();
    f.run(10_000_000).unwrap();
    output.extend(f.take_audio());
    if kind == "midi" {
        f.midi(&[0x91, 69, 100]).unwrap();
    }
    for _ in 0..100 {
        if let Err(e) = f.run(1_000_000) {
            panic!("{e}; {}", f.stats());
        }
        output.extend(f.take_audio());
        if output.len() >= 24_000 {
            break;
        }
    }
    let peak = output.iter().fold(0f32, |a, b| a.max(b.abs()));
    let rms =
        (output.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>() / output.len() as f64).sqrt();
    assert!(output.iter().all(|s| s.is_finite()), "non-finite audio");
    println!(
        "{kind}: samples={} peak={peak:.6} rms={rms:.6} wall={:?}; {}",
        output.len(),
        start.elapsed(),
        f.stats()
    );
    let mut wav = Vec::new();
    let size = (output.len() * 4) as u32;
    wav.extend(b"RIFF");
    wav.extend((size + 36).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend(3u16.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(48_014u32.to_le_bytes());
    wav.extend((48_014u32 * 4).to_le_bytes());
    wav.extend(4u16.to_le_bytes());
    wav.extend(32u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend(size.to_le_bytes());
    for s in output {
        wav.extend(s.to_le_bytes());
    }
    fs::write(format!("/private/tmp/synthphone-{kind}.wav"), wav).unwrap();
}
