//! Profile real ARM fallback instructions under continuous codec microphone input.
use sha2::{Digest, Sha256};
use std::{fs, time::Instant};
use virtual_daisy_web::{reference_image, Firmware};
fn main() {
    let path = std::env::args().nth(1).expect("output PC profile path");
    let mode = std::env::args().nth(2).unwrap_or_default();
    let all = matches!(mode.as_str(), "all" | "fallback-all");
    let mut firmware = Firmware::new(reference_image()).unwrap();
    firmware.handset(true).unwrap();
    if mode == "all" {
        firmware.start_profile();
    } else {
        firmware.start_fallback_profile();
    }
    let start = Instant::now();
    let mut phase = 0f64;
    let mut samples = 0;
    let mut peak = 0f32;
    let mut profile_mode = 0usize;
    let mut next_mode = 2_000_000_000u64;
    let mut midi_started = false;
    let mut next_drum = 750_000_000u64;
    let mut release_at = None;
    while firmware.virtual_time_ns() < if all { 12_000_000_000 } else { 4_000_000_000 } {
        if all {
            let now = firmware.virtual_time_ns();
            if now > 500_000_000 && !midi_started {
                firmware
                    .midi(&[
                        0x90, 69, 100, 0x91, 48, 100, 52, 90, 55, 90, 59, 80, 60, 80, 64, 80, 67,
                        80, 71, 80, 0xb0, 80, 127, 81, 127,
                    ])
                    .unwrap();
                midi_started = true;
            }
            if now >= next_drum {
                firmware.midi(&[0x99, 36, 100, 42, 90]).unwrap();
                next_drum += 250_000_000;
            }
            if now >= next_mode && profile_mode < 5 {
                firmware.key(10, true).unwrap();
                release_at = Some(now + 50_000_000);
                profile_mode += 1;
                next_mode += 2_000_000_000;
            }
            if release_at.is_some_and(|end| now >= end) {
                firmware.key(10, false).unwrap();
                release_at = None;
            }
        }
        if firmware.pending_input() < 4096 {
            let input: Vec<f32> = (0..4096)
                .flat_map(|_| {
                    let angle = phase * std::f64::consts::TAU;
                    let sample = if all {
                        (angle.sin() * 0.1
                            + (angle * 2.).sin() * 0.035
                            + (angle * 3.).sin() * 0.016) as f32
                    } else {
                        angle.sin() as f32 * 0.16
                    };
                    let hz = if all {
                        150. + 65.
                            * (firmware.virtual_time_ns() as f64 / 1e9
                                * 0.87
                                * std::f64::consts::TAU)
                                .sin()
                    } else {
                        220.
                    };
                    phase = (phase + hz / 48187.487053) % 1.;
                    [0., sample]
                })
                .collect();
            firmware.input_stereo(&input).unwrap();
        }
        firmware.run_for(100_000, 2_000_000).unwrap();
        let output = firmware.take_audio();
        samples += output.len();
        peak = output.iter().fold(peak, |a, b| a.max(b.abs()));
    }
    let mut profile = format!(
        "# Original ARM PC and execution count; {}\n# SHA-256: {:x}; {}\n",
        if all {
            "six profiles, active pitch/formant, vocal harmonics, eight MIDI voices and drums"
        } else {
            "fallback after register-local translation"
        },
        Sha256::digest(reference_image()),
        if mode == "all" {
            "raw interpreter visits"
        } else {
            "misses after register-local translation"
        }
    );
    for (index, count) in firmware.profile_counts().iter().enumerate() {
        if *count != 0 {
            profile.push_str(&format!("{:08x} {count}\n", 0x08000000 + index * 2));
        }
    }
    fs::write(path, profile).unwrap();
    println!(
        "wall={:?} samples={samples} peak={peak}; {}",
        start.elapsed(),
        firmware.stats()
    );
}
