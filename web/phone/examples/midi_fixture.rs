//! A small synthetic file for exercising the browser file-picker and UART path.
fn main() {
    let track = [
        0, 0x91, 69, 100, 0x83, 0x60, 0x81, 69, 0, 0x83, 0x60, 0x91, 72, 90, 0x83, 0x60, 0x81, 72,
        0, 0, 0xff, 0x2f, 0,
    ];
    let mut bytes = b"MThd\0\0\0\x06\0\0\0\x01\x01\xe0MTrk".to_vec();
    bytes.extend((track.len() as u32).to_be_bytes());
    bytes.extend(track);
    std::fs::write(std::env::args().nth(1).expect("output.mid"), bytes).unwrap();
}
