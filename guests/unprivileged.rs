//! Real unprivileged transfers across MPU permissions and alignment settings.
#![no_std]
#![no_main]
use core::{
    arch::{asm, global_asm},
    ptr::{read_volatile, write_volatile},
};
// UNPRIVILEGED_CASES
const RESULTS: *mut u32 = 0x20019800 as *mut u32;
const CURRENT: *mut u32 = 0x20019000 as *mut u32;
const BASE: u32 = 0x24020004;
global_asm!(
    r#"
.syntax unified
.thumb
.section .vectors,"a",%progbits
.word 0x2001f000,probe_main,probe_unexpected
.rept 4
.word probe_fault
.endr
.rept 9
.word probe_unexpected
.endr
.rept 160
.word probe_unexpected
.endr
.section .text.unprivileged_case,"ax",%progbits
.global unprivileged_case
.thumb_func
unprivileged_case:
push {{r4-r11,lr}}
ldr r3,=0x20019000
ldr r0,[r3,#20]
ldr r1,[r3,#24]
ldr r2,=0xa80a0000
msr APSR_nzcvqg,r2
ldr r3,=0x7001
blx r3
mrs r2,APSR
ldr r3,=0x20019000
stmia r3,{{r0-r2}}
pop {{r4-r11,pc}}
.ltorg
.global probe_fault
.thumb_func
probe_fault:
ldr r0,=0xe000ed28
ldr r1,[r0]
ldr r2,=0x20019000
str r1,[r2,#12]
tst r1,#128
ite ne
ldrne r3,[r0,#12]
moveq r3,#0
str r3,[r2,#16]
str r1,[r0]
ldr r0,[sp,#24]
adds r0,#4
str r0,[sp,#24]
bx lr
.global probe_unexpected
.thumb_func
probe_unexpected:
ldr r0,=0x20019800
mrs r1,ipsr
str r1,[r0,#60]
1: wfi
b 1b
.ltorg
"#
);
extern "C" {
    fn unprivileged_case();
}
unsafe fn barriers() {
    asm!("dsb", "isb", options(nostack));
}
unsafe fn run(opcode: u32, ap: u32, trap: bool, stir: bool, user_stir: bool, count: usize) {
    write_volatile(0xe000ed94 as *mut u32, 0);
    barriers();
    for i in 0..8u32 {
        let index = (opcode & 255) + 3 + i;
        let byte =
            (0x80008000u32.rotate_right((index % 4) * 8) as u8) ^ (index.wrapping_mul(37) as u8);
        write_volatile((BASE + (opcode & 255) - 1 + i) as *mut u8, byte);
    }
    for i in 0..7 {
        write_volatile(CURRENT.add(i), 0);
    }
    write_volatile(CURRENT.add(5), if stir { 5 } else { 0x80008000 });
    write_volatile(CURRENT.add(6), if stir { 0xe000ef00 } else { BASE });
    write_volatile(0x7000 as *mut u16, (opcode >> 16) as u16);
    write_volatile(0x7002 as *mut u16, opcode as u16);
    write_volatile(0x7004 as *mut u16, 0x4770);
    write_volatile(
        0xe000ed14 as *mut u32,
        0x200 | if trap { 8 } else { 0 } | if user_stir { 2 } else { 0 },
    );
    write_volatile(0xe000ed98 as *mut u32, 0);
    write_volatile(0xe000ed9c as *mut u32, BASE & !255);
    write_volatile(0xe000eda0 as *mut u32, (ap << 24) | (1 << 19) | 15);
    write_volatile(0xe000ed94 as *mut u32, if stir { 0 } else { 5 });
    barriers();
    unprivileged_case();
    write_volatile(0xe000ed94 as *mut u32, 0);
    barriers();
    let bytes: [u8; 8] = core::array::from_fn(|i| {
        read_volatile((BASE + (opcode & 255) - 1 + i as u32) as *const u8)
    });
    let lo = if stir {
        read_volatile(0xe000e200 as *const u32)
    } else {
        u32::from_le_bytes(bytes[..4].try_into().unwrap())
    };
    let hi = if stir {
        (read_volatile(0xe000ed04 as *const u32) >> 12) & 511
    } else {
        u32::from_le_bytes(bytes[4..].try_into().unwrap())
    };
    let row = [
        opcode,
        read_volatile(CURRENT),
        lo,
        hi,
        read_volatile(CURRENT.add(2)),
        read_volatile(CURRENT.add(3)),
        read_volatile(CURRENT.add(4)),
        read_volatile(CURRENT.add(1)),
    ];
    for (i, word) in row.into_iter().enumerate() {
        write_volatile(RESULTS.add(16 + count * 8 + i), word);
    }
    if stir {
        write_volatile(0xe000e280 as *mut u32, 1 << 5);
    }
}
#[no_mangle]
pub unsafe extern "C" fn probe_main() -> ! {
    for i in 0..608 {
        write_volatile(RESULTS.add(i), 0);
    }
    write_volatile(0xe000ed24 as *mut u32, 0x70000);
    let mut count = 0;
    for ap in [0, 1, 2, 3, 5, 6, 7] {
        for [opcode, _, _, _, _] in UNPRIVILEGED_CASES {
            if opcode & 255 == 0 {
                run(opcode, ap, false, false, false, count);
                count += 1;
            }
        }
    }
    for trap in [false, true] {
        for [opcode, _, _, _, _] in UNPRIVILEGED_CASES {
            if opcode & 255 == 1 {
                run(opcode, 3, trap, false, false, count);
                count += 1;
            }
        }
    }
    for user in [false, true] {
        run(0xf8410e00, 3, false, true, user, count);
        count += 1;
    }
    write_volatile(RESULTS.add(1), count as u32);
    write_volatile(RESULTS, 0xab00f027);
    loop {
        asm!("wfi", options(nostack));
    }
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {
        unsafe {
            asm!("wfi", options(nostack));
        }
    }
}
