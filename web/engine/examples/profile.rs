//! Capture hot ARM instruction addresses without translating guest DSP routines.
use virtual_daisy_web::{reference_image, Firmware};
fn main() {
    let mut f = Firmware::new(reference_image()).unwrap();
    f.start_profile();
    f.input(
        &(0..96000)
            .map(|n| (n as f32 * 220. * std::f32::consts::TAU / 48_014.312).sin() * 0.2)
            .collect::<Vec<_>>(),
    );
    for stage in 0..8 {
        if stage == 2 {
            f.midi(&[0x91, 60, 100, 64, 90, 67, 80]).unwrap();
        }
        if stage == 3 {
            f.midi(&[0x99, 36, 120, 38, 110, 42, 100]).unwrap();
        }
        if stage == 4 {
            f.encoder_button(true).unwrap();
        }
        if stage == 5 {
            f.encoder_button(false).unwrap();
            f.encoder(3);
        }
        if stage == 6 {
            f.key(10, true).unwrap();
        }
        for _ in 0..25 {
            f.run(1_000_000).unwrap();
            f.take_audio();
        }
    }
    let mut data = String::from("# ARM PC and interpreter visits; original bundled image only\n");
    for (index, count) in f.profile_counts().iter().enumerate() {
        if *count > 10 {
            data.push_str(&format!("{:08x} {count}\n", 0x08000000 + index * 2));
        }
    }
    std::fs::write(std::env::args().nth(1).expect("output path"), data).unwrap();
    println!("{}", f.stats());
}
