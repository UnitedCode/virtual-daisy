fn main() {
    let path = std::env::args().nth(1).expect("raw firmware .bin");
    let image = std::fs::read(path).unwrap();
    let mut firmware = virtual_daisy_web::Firmware::new(&image).unwrap();
    let now = std::time::Instant::now();
    for _ in 0..20 {
        match firmware.run(1_000_000) {
            Ok(state) => println!(
                "{state:?}; {}; elapsed={:?}",
                firmware.stats(),
                now.elapsed()
            ),
            Err(error) => {
                eprintln!("{error}; {}", firmware.stats());
                std::process::exit(1);
            }
        }
    }
}
