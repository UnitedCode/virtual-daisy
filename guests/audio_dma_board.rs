// Register helpers for the Rev5 WM8731 loopback guests.
use core::ptr::{read_volatile, write_volatile};

const RCC: u32 = 0x58024400;
const SAI: u32 = 0x40015800;
const I2C2: u32 = 0x40005800;
const DMA1: u32 = 0x40020000;
const DMAMUX1: u32 = 0x40020800;

fn read(address: u32) -> u32 {
    unsafe { read_volatile(address as *const u32) }
}

fn write(address: u32, value: u32) {
    unsafe { write_volatile(address as *mut u32, value) }
}

fn configure_codec() {
    write(RCC + 0xe8, read(RCC + 0xe8) | (1 << 22));
    write(I2C2, 1);
    for (reg, value) in [
        (0, 23),
        (1, 23),
        (4, 0x12),
        (5, 0),
        (6, 0x42),
        (7, 9),
        (8, 0),
        (9, 1),
    ] {
        write(I2C2 + 4, (0x1a << 1) | (2 << 16) | (1 << 25) | (1 << 13));
        while read(I2C2 + 0x18) & (1 << 1) == 0 {}
        write(I2C2 + 0x28, (reg << 1) | ((value >> 8) & 1));
        while read(I2C2 + 0x18) & (1 << 1) == 0 {}
        write(I2C2 + 0x28, value & 255);
        while read(I2C2 + 0x18) & (1 << 5) == 0 {}
        write(I2C2 + 0x1c, 1 << 5);
    }
}
