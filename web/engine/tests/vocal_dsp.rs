//! Test a real vocal effect through the original ARM firmware and MIDI UART.
use virtual_daisy_web::{reference_image, Firmware};
fn render(up: bool) -> Vec<f32> {
    let mut f = Firmware::new(reference_image()).unwrap();
    f.input(
        &(0..48_000)
            .map(|n| (n as f32 * 220. * std::f32::consts::TAU / 48_014.312).sin() * 0.2)
            .collect::<Vec<_>>(),
    );
    f.run(5_000_000).unwrap();
    f.midi(&[0xb0, 80, if up { 127 } else { 64 }]).unwrap();
    let mut out = Vec::new();
    for _ in 0..500 {
        f.run(500_000).unwrap();
        out.extend(f.take_audio());
        if out.len() >= 16_000 {
            break;
        }
    }
    assert!(out.len() >= 16_000, "firmware audio stalled: {}", f.stats());
    assert!(out.iter().all(|s| s.is_finite()));
    out
}
fn energy(samples: &[f32], hz: f64) -> f64 {
    let samples = &samples[samples.len() - 2048..];
    let mut real = 0.;
    let mut imaginary = 0.;
    for (n, sample) in samples.iter().enumerate() {
        let phase = n as f64 * hz * std::f64::consts::TAU / 48_014.312;
        real += f64::from(*sample) * phase.cos();
        imaginary += f64::from(*sample) * phase.sin();
    }
    (real * real + imaginary * imaginary) / (samples.len() as f64).powi(2)
}
#[test]
fn midi_octave_control_changes_the_processed_mic_pitch() {
    let dry = render(false);
    let shifted = render(true);
    let a = (energy(&dry, 220.), energy(&dry, 440.));
    let b = (energy(&shifted, 220.), energy(&shifted, 440.));
    assert!(a.0 > a.1 * 4., "normal pitch 220/440 Hz energy: {a:?}");
    assert!(b.1 > b.0 * 4., "octave-up pitch 220/440 Hz energy: {b:?}");
}
