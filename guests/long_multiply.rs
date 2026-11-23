//! Standalone register/alias probe; exported source needs no repository includes.
#![no_std]
#![no_main]
use core::{
    arch::{asm, global_asm},
    ptr::{read_volatile, write_volatile},
};
const RESULTS: *mut u32 = 0x20018800 as *mut u32;
const DATA: *mut u32 = 0x2001b000 as *mut u32;
const REGISTERS: [u32; 14] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 14];
// LONG_MULTIPLY_CASES
global_asm!(
    r#"
.syntax unified
.thumb
.section .vectors,"a",%progbits
.word 0x2001f000, probe_main
.rept 14
.word probe_unexpected
.endr
.section .text.long_multiply_case,"ax",%progbits
.global long_multiply_case
.thumb_func
long_multiply_case:
push {{r4-r11,lr}}
ldr r0, =0x2001b080
str sp, [r0]
ldr r0, =0x2001b03c
ldr r0, [r0]
msr APSR_nzcvqg, r0
ldr sp, =0x2001b000
ldmia.w sp, {{r0-r12,lr}}
ldr pc, =0xe001
.global long_multiply_continue
.thumb_func
long_multiply_continue:
stmia.w sp, {{r0-r12,lr}}
mrs r0, APSR
str r0, [sp, #56]
ldr r0, =0x2001b080
ldr r0, [r0]
mov sp, r0
pop {{r4-r11,pc}}
.ltorg
.section .text.probe_unexpected,"ax",%progbits
.global probe_unexpected
.thumb_func
probe_unexpected:
ldr r1, =0x20018800
mrs r0, ipsr
str r0, [r1, #124]
mov r0, sp
str r0, [r1, #108]
mov r0, lr
str r0, [r1, #112]
ldr r0, [sp, #24]
str r0, [r1, #116]
ldr r0, [sp, #96]
str r0, [r1, #120]
ldr r0, =0xe000ed28
ldr r2, [r0]
str r2, [r1, #100]
ldr r2, [r0, #4]
str r2, [r1, #104]
1: wfi
b 1b
"#
);
extern "C" {
    fn long_multiply_case();
    fn long_multiply_continue();
}

#[inline(never)]
unsafe fn matrix() -> bool {
    let code = 0xe000usize;
    write_volatile((code + 4) as *mut u16, 0xf8df);
    write_volatile((code + 6) as *mut u16, 0xf000);
    write_volatile(
        (code + 8) as *mut u32,
        long_multiply_continue as *const () as u32 | 1,
    );
    let mut count = 0;
    let mut hash = 0xcbf29ce484222325u64;
    for (case, [base, input, control, lo, hi, status]) in LONG_CASES.into_iter().enumerate() {
        for low in 0..14 {
            for high in 0..14 {
                if low == high {
                    continue;
                }
                for source in 0..14 {
                    let opcode = (base & !0x0000ff0f)
                        | (REGISTERS[low] << 12)
                        | (REGISTERS[high] << 8)
                        | REGISTERS[source];
                    let mut expected: [u32; 14] =
                        core::array::from_fn(|i| 0xdeadbeef ^ (REGISTERS[i] * 0x01010101));
                    expected[low] = input;
                    expected[high] = input;
                    expected[2] = input;
                    expected[source] = input;
                    for (i, word) in expected.into_iter().enumerate() {
                        write_volatile(DATA.add(i), word);
                    }
                    expected[low] = lo;
                    expected[high] = hi;
                    write_volatile(DATA.add(15), control);
                    write_volatile(code as *mut u16, (opcode >> 16) as u16);
                    write_volatile((code + 2) as *mut u16, opcode as u16);
                    write_volatile(RESULTS.add(1), case as u32);
                    write_volatile(RESULTS.add(2), opcode);
                    write_volatile(RESULTS.add(5), count);
                    asm!("dsb", "isb", options(nostack));
                    long_multiply_case();
                    for i in 0..15 {
                        let actual = read_volatile(DATA.add(i));
                        let wanted = if i == 14 { status } else { expected[i] };
                        if actual != wanted {
                            write_volatile(RESULTS, 0xbad0f023);
                            write_volatile(RESULTS.add(3), i as u32);
                            write_volatile(RESULTS.add(4), actual);
                            write_volatile(RESULTS.add(5), wanted);
                            return false;
                        }
                        for byte in actual.to_le_bytes() {
                            hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
                        }
                    }
                    count += 1;
                }
            }
        }
    }
    write_volatile(RESULTS.add(27), count);
    write_volatile(RESULTS.add(28), hash as u32);
    write_volatile(RESULTS.add(29), (hash >> 32) as u32);
    write_volatile(RESULTS.add(30), 1);
    true
}
#[no_mangle]
pub unsafe extern "C" fn probe_main() -> ! {
    for i in 0..32 {
        write_volatile(RESULTS.add(i), 0);
    }
    if matrix() {
        write_volatile(RESULTS, 0xab00f023);
    }
    loop {
        asm!("wfi", options(nostack));
    }
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
