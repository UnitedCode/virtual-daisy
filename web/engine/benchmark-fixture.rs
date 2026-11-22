//! Deterministic codec stimuli. This is test input, never an instrument engine.
use std::{
    fs::File,
    io::{BufWriter, Write},
};
fn main() -> std::io::Result<()> {
    let mut output = BufWriter::new(File::create(
        std::env::args().nth(1).expect("output .f32 path"),
    )?);
    if std::env::args().nth(2).as_deref() == Some("midi") {
        let mut track = vec![
            0, 0xff, 0x51, 3, 7, 0xa1, 0x20, 0, 0x90, 69, 100, 0, 0xb0, 80, 127, 0, 0xb0, 81, 127,
        ];
        let notes = [48, 52, 55, 59, 60, 64, 67, 71];
        for note in notes {
            track.extend_from_slice(&[0, 0x91, note, 90]);
        }
        for n in 0..480 {
            track.extend_from_slice(&[120, 0x99, [36, 42, 38, 49][n % 4], 100]);
        }
        track.extend_from_slice(&[0, 0x80, 69, 0]);
        for note in notes {
            track.extend_from_slice(&[0, 0x81, note, 0]);
        }
        track.extend_from_slice(&[0, 0xff, 0x2f, 0]);
        output.write_all(b"MThd\0\0\0\x06\0\0\0\x01\x01\xe0MTrk")?;
        output.write_all(&(track.len() as u32).to_be_bytes())?;
        output.write_all(&track)?;
        return output.flush();
    }
    let voice = std::env::args().nth(2).as_deref() == Some("voice");
    let mut phase = 0f64;
    for n in 0..48187 {
        let time = n as f64 / 48187.487053;
        let hz = if voice {
            150. + 70. * (time * 2.2 * std::f64::consts::TAU).sin()
        } else {
            220.
        };
        let angle = phase * std::f64::consts::TAU;
        let sample = if voice {
            angle.sin() * 0.1 + (2. * angle).sin() * 0.035 + (3. * angle).sin() * 0.016
        } else {
            angle.sin() * 0.16
        } as f32;
        phase = (phase + hz / 48187.487053) % 1.;
        output.write_all(&0f32.to_le_bytes())?;
        output.write_all(&sample.to_le_bytes())?;
    }
    output.flush()
}
