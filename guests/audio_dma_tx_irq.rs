//! Rev5 loopback driven by the transmit DMA interrupt.
#![no_std]
#![no_main]

use core::arch::asm;

include!("audio_dma_board.rs");

const RX_RING: u32 = 0x30000000;
const TX_RING: u32 = 0x30004000;
const RING_WORDS: u32 = 8;
const MASTER: u32 = (4 << 20) | (6 << 5) | (1 << 17) | (1 << 16);

#[repr(C)]
#[derive(Clone, Copy)]
union Vector {
    stack: u32,
    handler: extern "C" fn() -> !,
    irq: extern "C" fn(),
}

#[used]
#[link_section = ".vector_table"]
static VECTORS: [Vector; 29] = {
    let mut table = [Vector { handler: stop }; 29];
    table[0] = Vector { stack: 0x20020000 };
    table[1] = Vector { handler: Reset };
    table[28] = Vector { irq: copy_input };
    table
};

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
    // Quarter-full RX leaves one received word pending. The control enables
    // the request at one word while leaving TX and the IRQ unchanged.
    write(SAI + 8, u32::from(!cfg!(rx_at_one_word)));
    write(SAI + 0x28, 1); // TX request below two words.
    for slot in 0..RING_WORDS {
        write(TX_RING + slot * 4, 0);
    }
    let circular_word_stream = (2 << 13) | (2 << 11) | (1 << 10) | (1 << 8) | (2 << 16) | 1;
    for (number, request, data, memory, transmit) in [
        (0, 87, SAI + 0x20, RX_RING, false),
        (1, 88, SAI + 0x40, TX_RING, true),
    ] {
        write(DMAMUX1 + number * 4, request);
        let stream = DMA1 + 0x10 + number * 0x18;
        write(stream + 4, RING_WORDS);
        write(stream + 8, data);
        write(stream + 12, memory);
        let direction = if transmit {
            (1 << 6) | (1 << 4) | (1 << 3)
        } else {
            0
        };
        write(stream, circular_word_stream | direction);
    }
    write(0xe000_e100, 1 << 12);
    write(SAI + 0x24, MASTER | (1 << 10) | 2); // Synchronous B transmitter.
    while read(SAI + 0x38) & (7 << 16) == 0 {} // Match HAL's nonempty wait.
    write(SAI + 4, MASTER | 1); // A receiver provides the frame clock.
    write(SAI + 0x40, 0);
    write(SAI + 0x40, 0);
    stop()
}

extern "C" fn copy_input() {
    let status = read(DMA1);
    for (flag, start) in [(1 << 10, 0), (1 << 11, RING_WORDS / 2)] {
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

extern "C" fn stop() -> ! {
    loop {
        unsafe { asm!("wfi") }
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    stop()
}
