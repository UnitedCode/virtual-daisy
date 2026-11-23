//! Check both SysTick clock sources against the independent core-cycle counter.
#![no_std]
#![no_main]
use core::{arch::{asm, global_asm}, ptr::{read_volatile, write_volatile}};
const RESULTS: *mut u32 = 0x20019f00 as *mut u32;
unsafe fn read(a: u32) -> u32 { read_volatile(a as *const u32) }
unsafe fn write(a: u32, v: u32) { write_volatile(a as *mut u32, v); }
unsafe fn record(i: usize, v: u32) { write_volatile(RESULTS.add(i), v); }
global_asm!(r#"
    .syntax unified
    .thumb
    .section .vectors,"a",%progbits
    .word 0x2001f000,probe_main
    .rept 14
    .word probe_unexpected
    .endr
"#);
#[no_mangle]
pub unsafe extern "C" fn probe_main() -> ! {
    for i in 0..32 { record(i, 0); }
    write(0xe000edfc, 1 << 24);
    write(0xe0001fb0, 0xc5acce55);
    write(0xe0001000, 1);
    write(0xe0001004, 0);
    for (phase, source) in [(0, 0), (1, 4)] {
        write(0xe000e010, 0);
        write(0xe000e014, 0xffffff);
        write(0xe000e018, 0);
        write(0xe000e010, source | 1);
        while read(0xe000e018) == 0 {}
        let start = read(0xe0001004);
        let before = read(0xe000e018);
        while read(0xe0001004).wrapping_sub(start) < 8192 {}
        let after = read(0xe000e018);
        let end = read(0xe0001004);
        let at = 1 + phase * 5;
        for (n, v) in [read(0xe000e010), before, after, start, end].into_iter().enumerate() {
            record(at + n, v);
        }
    }
    write(0xe000e010, 0);
    record(0, 0xab);
    loop { asm!("wfi"); }
}
#[no_mangle]
pub unsafe extern "C" fn probe_unexpected() -> ! {
    record(31, 1);
    loop { asm!("wfi"); }
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { loop {} }
