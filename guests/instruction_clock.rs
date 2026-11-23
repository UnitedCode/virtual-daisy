//! DWT measurements of literal instruction sequences, executed from ITCM.
#![no_std]
#![no_main]
use core::{
    arch::{asm, global_asm},
    ptr::{read_volatile, write_volatile},
};
const RESULTS: *mut u32 = 0x2001_9000 as *mut u32;
global_asm!(
    r#"
.syntax unified
.thumb
.section .vectors,"a",%progbits
.word 0x2001f000, probe_main
.rept 9
.word probe_unexpected
.endr
.word svc_handler
.rept 4
.word probe_unexpected
.endr
.section .text.bench,"ax",%progbits
.thumb_func
svc_handler:
bx lr
.macro BEGIN name
.global \name
.thumb_func
\name:
ldr r2, =0xe0001004
ldr r0, [r2]
.endm
.macro END
ldr r1, [r2]
subs r0, r1, r0
bx lr
.endm
BEGIN bench_empty
END
BEGIN bench_nop1
nop
END
BEGIN bench_nop128
.rept 128
nop
.endr
END
BEGIN bench_branch128
.rept 128
b.n 1f
nop
1: nop
.endr
END
BEGIN bench_mul128
movs r1, #111
movs r3, #3
.rept 128
mul r12, r1, r3
.endr
END
BEGIN bench_div128
movs r1, #111
movs r3, #3
.rept 128
udiv r12, r1, r3
.endr
END
BEGIN bench_tcm_load128
ldr r1, =0x20018000
.rept 128
ldr r3, [r1]
.endr
END
BEGIN bench_axi_load128
ldr r1, =0x24000000
.rept 128
ldr r3, [r1]
.endr
END
BEGIN bench_tcm_store128
ldr r1, =0x20018000
movs r3, #123
.rept 128
str r3, [r1]
.endr
END
BEGIN bench_tcm_pair128
ldr r1, =0x20018000
.rept 128
ldrd r3, r12, [r1]
.endr
END
BEGIN bench_tcm_multiple128
ldr r1, =0x20018000
.rept 128
ldmia.w r1, {{r3, r12}}
.endr
END
BEGIN bench_svc8
.rept 8
svc #0
.endr
END
.ltorg
"#
);
unsafe extern "C" {
    fn bench_empty() -> u32;
    fn bench_nop1() -> u32;
    fn bench_nop128() -> u32;
    fn bench_branch128() -> u32;
    fn bench_mul128() -> u32;
    fn bench_div128() -> u32;
    fn bench_tcm_load128() -> u32;
    fn bench_axi_load128() -> u32;
    fn bench_tcm_store128() -> u32;
    fn bench_tcm_pair128() -> u32;
    fn bench_tcm_multiple128() -> u32;
    fn bench_svc8() -> u32;
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn probe_main() -> ! {
    for i in 0..32 {
        write_volatile(RESULTS.add(i), 0);
    }
    write_volatile(0xe000ed08 as *mut u32, 0x6000);
    write_volatile(0xe000edfc as *mut u32, 1 << 24);
    write_volatile(0xe0001fb0 as *mut u32, 0xc5acce55);
    write_volatile(0xe0001000 as *mut u32, 1);
    write_volatile(0xe0001004 as *mut u32, 0);
    for (i, function) in [
        bench_empty,
        bench_nop1,
        bench_nop128,
        bench_branch128,
        bench_mul128,
        bench_div128,
        bench_tcm_load128,
        bench_axi_load128,
        bench_tcm_store128,
        bench_tcm_pair128,
        bench_tcm_multiple128,
        bench_svc8,
    ]
    .into_iter()
    .enumerate()
    {
        write_volatile(RESULTS.add(i + 1), function());
    }
    write_volatile(RESULTS.add(20), read_volatile(0xe000ed28 as *const u32));
    write_volatile(RESULTS.add(21), read_volatile(0xe000ed2c as *const u32));
    write_volatile(RESULTS, 0xab00_c10c);
    loop {
        asm!("wfi");
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn probe_unexpected() -> ! {
    let ipsr: u32;
    asm!("mrs {}, ipsr",out(reg)ipsr);
    write_volatile(RESULTS.add(31), ipsr);
    loop {
        asm!("wfi");
    }
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
