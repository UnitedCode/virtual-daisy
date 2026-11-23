//! Host-driven sleep checkpoints; no polling loop substitutes for WFE or WFI.
#![no_std]
#![no_main]
use core::{
    arch::{asm, global_asm},
    ptr::write_volatile,
};
global_asm!(
    r#"
.syntax unified
.thumb
.section .vectors,"a",%progbits
.word 0x2001f000, probe_main
.rept 14
.word probe_unexpected
.endr
.rept 5
.word probe_unexpected
.endr
.word probe_irq
.section .text.events,"ax",%progbits
.global probe_body
.thumb_func
probe_body:
cpsid i
ldr r0, =0x20019800
ldr r1, =0xe000ed10
movs r2, #16
str r2, [r1]
ldr r2, =0xa80a0000
msr APSR_nzcvqg, r2
sev
sev
wfe
mrs r2, APSR
str r2, [r0, #12]
movs r2, #1
str r2, [r0, #4]
wfe
movs r2, #2
str r2, [r0, #4]
sev
wfe
movs r2, #3
str r2, [r0, #4]
wfe
movs r2, #4
str r2, [r0, #4]
sev
wfe
movs r2, #0
str r2, [r1]
movs r2, #5
str r2, [r0, #4]
wfe
movs r2, #6
str r2, [r0, #4]
sev
wfe
ldr r1, =0xe000e100
movs r2, #32
str r2, [r1]
movs r2, #7
str r2, [r0, #4]
wfe
movs r2, #8
str r2, [r0, #4]
sev
wfe
ldr r1, =0xe000e280
movs r2, #32
str r2, [r1]
movs r2, #9
str r2, [r0, #4]
wfi
movs r2, #10
str r2, [r0, #4]
// The event received during WFI is still latched.
wfe
movs r2, #11
str r2, [r0, #4]
sev
wfe
// The pending IRQ runs on unmask; exception return supplies the next event.
cpsie i
isb
wfe
movs r2, #12
str r2, [r0, #4]
ldr r2, =0xab00f026
str r2, [r0]
cpsid i
1: wfi
b 1b
.ltorg
.global probe_irq
.thumb_func
probe_irq:
ldr r0, =0x20019800
ldr r1, [r0, #8]
adds r1, #1
str r1, [r0, #8]
bx lr
.global probe_unexpected
.thumb_func
probe_unexpected:
ldr r0, =0x20019800
mrs r1, ipsr
str r1, [r0, #124]
2: wfi
b 2b
.ltorg
"#
);
extern "C" {
    fn probe_body() -> !;
}
#[no_mangle]
pub unsafe extern "C" fn probe_main() -> ! {
    for i in 0..32 {
        write_volatile((0x20019800 as *mut u32).add(i), 0);
    }
    probe_body()
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {
        unsafe {
            asm!("wfi", options(nostack));
        }
    }
}
