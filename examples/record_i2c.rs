//! Attach an explicit I²C recording sink to a firmware run.
//! This records transactions; it does not simulate a display or other device.
use std::{cell::RefCell, error::Error, fs, rc::Rc};
use virtual_daisy::{
    board::Board,
    machine::{Carrier, Machine},
};

type Packets = Rc<RefCell<Vec<(u8, u8, Vec<u8>)>>>;
struct Recorder {
    bus: u8,
    address: u8,
    packets: Packets,
}
impl Carrier for Recorder {
    fn i2c_write(
        &mut self,
        bus: u8,
        address: u8,
        bytes: &[u8],
    ) -> Result<(), virtual_daisy::machine::Error> {
        if (bus, address) != (self.bus, self.address) {
            return Err("I2C recording sink received a different bus/address".into());
        }
        self.packets
            .borrow_mut()
            .push((bus, address, bytes.to_vec()));
        Ok(())
    }
}
fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 5 {
        return Err("Usage: cargo run --release --example record_i2c -- FIRMWARE.elf rev5 BUS ADDRESS_HEX DURATION_MS".into());
    }
    let board: Board = args[1].parse()?;
    let bus: u8 = args[2].parse()?;
    let address = u8::from_str_radix(args[3].trim_start_matches("0x"), 16)?;
    let ms: u64 = args[4].parse()?;
    if !(1..=2).contains(&bus) || address > 127 || !(1..=10_000).contains(&ms) {
        return Err("bus must be 1..2, address 00..7f, duration 1..10000 ms".into());
    }
    let packets: Packets = Rc::default();
    let mut machine = Machine::from_elf(board, &fs::read(&args[0])?)?;
    machine.attach(Box::new(Recorder {
        bus,
        address,
        packets: packets.clone(),
    }));
    machine.run_until(ms * 1_000_000, 50_000_000)?;
    println!(
        "elapsed_ns={}\ni2c_packets={}\naudio_frames={}\nfinal_pc=0x{:08x}\nsample_rate={:?}",
        machine.time_ns(),
        packets.borrow().len(),
        machine.take_audio().len(),
        machine.registers()[15],
        machine.sample_rate()
    );
    Ok(())
}
