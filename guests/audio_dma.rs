//! Rev5 stereo loopback with both SAI DMA streams active.
#![no_std]
#![no_main]

use core::{
    arch::asm,
    ptr::{read_volatile, write_volatile},
};

const RCC: u32 = 0x58024400;
const SAI: u32 = 0x40015800;
const I2C2: u32 = 0x40005800;
const DMA1: u32 = 0x40020000;
const DMAMUX1: u32 = 0x40020800;
const RX_RING: u32 = 0x30018000;
const TX_RING: u32 = 0x30018100;
const RING_WORDS: u32 = 32;
const MASTER: u32 = (4 << 20) | (6 << 5) | (1 << 17) | (1 << 16);

#[repr(C)]
#[derive(Clone, Copy)]
union Vector {
    stack: u32,
    handler: extern "C" fn() -> !,
}

#[used]
#[link_section = ".vector_table"]
static VECTORS: [Vector; 16] = {
    let mut table = [Vector { handler: stop }; 16];
    table[0] = Vector { stack: 0x20020000 };
    table[1] = Vector { handler: Reset };
    table
};

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

#[no_mangle]
extern "C" fn Reset() -> ! {
    configure_codec();
    write(RCC + 0xf0, read(RCC + 0xf0) | (1 << 22));
    write(RCC + 0xd8, read(RCC + 0xd8) | 1);
    write(RCC + 0x50, 4); // HSI64 / 4 / 256 gives 62,500 frames per second.
    for block in [SAI, SAI + 32] {
        write(block + 0xc, 63 | (31 << 8) | (1 << 16) | (1 << 18));
        write(block + 0x10, 0x30180);
    }
    write(SAI + 8, 0); // RX request at one word.
    write(SAI + 0x28, 3); // TX request below six words.
    for slot in 0..RING_WORDS {
        write(TX_RING + slot * 4, 0);
    }
    let circular_word_stream = (2 << 13) | (2 << 11) | (1 << 10) | (1 << 8) | 1;
    for (number, request, data, memory, transmit) in [
        (0, 87, SAI + 0x20, RX_RING, false),
        (1, 88, SAI + 0x40, TX_RING, true),
    ] {
        write(DMAMUX1 + number * 4, request);
        let stream = DMA1 + 0x10 + number * 0x18;
        write(stream + 4, RING_WORDS);
        write(stream + 8, data);
        write(stream + 12, memory);
        write(stream, circular_word_stream | (u32::from(transmit) << 6));
    }
    write(SAI + 0x24, MASTER | (1 << 10) | 2); // Synchronous B transmitter.
    while read(SAI + 0x38) & 8 != 0 {} // Prime TX before starting its clock.
    write(SAI + 4, MASTER | 1); // A receiver provides the frame clock.
    loop {
        let status = read(DMA1);
        for (flag, start) in [(1 << 4, 0), (1 << 5, RING_WORDS / 2)] {
            if status & flag == 0 {
                continue;
            }
            write(DMA1 + 8, flag);
            for slot in start..start + RING_WORDS / 2 {
                let offset = slot * 4;
                write(TX_RING + offset, read(RX_RING + offset));
            }
        }
    }
}

extern "C" fn stop() -> ! {
    loop {
        unsafe { asm!("wfi") }
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    stop()
}
