//! Literal half-storage conversions, including direct binary64 rounding.
#![no_std]
#![no_main]
use core::{
    arch::{asm, global_asm},
    ptr::{read_volatile, write_volatile},
};
const RESULTS: *mut u32 = 0x20018c00 as *mut u32;
global_asm!(
    r#"
.syntax unified
.thumb
.section .vectors,"a",%progbits
.word 0x2001f000, probe_main
.rept 14
.word probe_unexpected
.endr
.section .text.probe_unexpected,"ax",%progbits
.global probe_unexpected
.thumb_func
probe_unexpected:
ldr r1, =0x20018c00
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
macro_rules! single {
    ($instruction:literal,$bits:expr,$control:expr) => {{
        let value:u32;let flags:u32;
        asm!("vmov s2, {input}","vmov s0, {canary}","vmsr fpscr, {control}",
            $instruction,"vmov {value}, s0","vmrs {flags}, fpscr",
            input=in(reg)$bits as u32,canary=in(reg)0xa5a55a5au32,control=in(reg)$control,
            value=out(reg)value,flags=out(reg)flags,out("s0")_,out("s2")_,options(nostack));
        (value,0,flags)
    }};
}
macro_rules! widen {
    ($instruction:literal,$bits:expr,$control:expr) => {{
        let lo:u32;let hi:u32;let flags:u32;
        asm!("vmov s2, {input}","vmsr fpscr, {control}",$instruction,
            "vmov {lo}, {hi}, d0","vmrs {flags}, fpscr",
            input=in(reg)$bits as u32,control=in(reg)$control,
            lo=out(reg)lo,hi=out(reg)hi,flags=out(reg)flags,out("d0")_,out("s2")_,options(nostack));
        (lo,hi,flags)
    }};
}
macro_rules! narrow {
    ($instruction:literal,$bits:expr,$control:expr) => {{
        let value:u32;let flags:u32;
        asm!("vmov d1, {il}, {ih}","vmov s0, {canary}","vmsr fpscr, {control}",
            $instruction,"vmov {value}, s0","vmrs {flags}, fpscr",
            il=in(reg)$bits as u32,ih=in(reg)($bits>>32) as u32,
            canary=in(reg)0xa5a55a5au32,control=in(reg)$control,
            value=out(reg)value,flags=out(reg)flags,out("s0")_,out("d1")_,options(nostack));
        (value,0,flags)
    }};
}
const CASES: [(u32, u64, u32, [u32; 3]); 8] = [
    (0, 0xa5a53c00, 0xa0000000, [0x3f800000, 0, 0xa0000000]),
    (1, 0xc0005a5a, 0xa0000000, [0xc0000000, 0, 0xa0000000]),
    (2, 0x3f801001, 0xa0000000, [0xa5a53c01, 0, 0xa0000010]),
    (3, 0x7f800000, 0xa0000000, [0x7c005a5a, 0, 0xa0000000]),
    (4, 0xa5a57fff, 0xa4000000, [0, 0x40fffc00, 0xa4000000]),
    (5, 0x7d015a5a, 0xa2000000, [0, 0x7ff80000, 0xa2000001]),
    (
        6,
        0x3ff0020000000001,
        0xa0000000,
        [0xa5a53c01, 0, 0xa0000010],
    ),
    (
        7,
        0x7ff0000000000000,
        0xa4000000,
        [0x7fff5a5a, 0, 0xa4000001],
    ),
];
unsafe fn execute(op: u32, bits: u64, control: u32) -> (u32, u32, u32) {
    match op {
        0 => single!("vcvtb.f32.f16 s0, s2", bits, control),
        1 => single!("vcvtt.f32.f16 s0, s2", bits, control),
        2 => single!("vcvtb.f16.f32 s0, s2", bits, control),
        3 => single!("vcvtt.f16.f32 s0, s2", bits, control),
        4 => widen!("vcvtb.f64.f16 d0, s2", bits, control),
        5 => widen!("vcvtt.f64.f16 d0, s2", bits, control),
        6 => narrow!("vcvtb.f16.f64 s0, d1", bits, control),
        _ => narrow!("vcvtt.f16.f64 s0, d1", bits, control),
    }
}
#[inline(never)]
unsafe fn register_matrix() -> bool {
    let code = 0x7000usize;
    write_volatile((code + 4) as *mut u16, 0x4770); // BX LR
    let bases = [
        0xeeb20a41u32,
        0xeeb20ac1,
        0xeeb30a41,
        0xeeb30ac1,
        0xeeb20b41,
        0xeeb20bc1,
        0xeeb30b41,
        0xeeb30bc1,
    ];
    let mut hash = 0xcbf29ce484222325u64;
    let mut count = 0u32;
    for (op, input, control, result) in CASES {
        let double = op >= 4;
        let to_half = op % 4 >= 2;
        for half in 0..32 {
            for full in 0..if double { 16 } else { 32 } {
                let mut before: [u32; 32] =
                    core::array::from_fn(|i| 0xdeadbeef ^ i as u32 * 0x01010101);
                if to_half {
                    before[half] = 0xa5a55a5a;
                    if double {
                        before[2 * full] = input as u32;
                        before[2 * full + 1] = (input >> 32) as u32;
                    } else {
                        before[full] = input as u32;
                    }
                } else {
                    before[half] = input as u32;
                }
                let mut expected = before;
                if to_half {
                    let mask = 0xffffu32 << (16 * (op & 1));
                    expected[half] = (expected[half] & !mask) | (result[0] & mask);
                } else if double {
                    expected[2 * full] = result[0];
                    expected[2 * full + 1] = result[1];
                } else {
                    expected[full] = result[0];
                }
                let encode = |reg: usize, wide: bool| {
                    if wide {
                        (reg as u32, 0)
                    } else {
                        ((reg >> 1) as u32, (reg & 1) as u32)
                    }
                };
                let (vd, d) = if to_half {
                    encode(half, false)
                } else {
                    encode(full, double)
                };
                let (vm, m) = if to_half {
                    encode(full, double)
                } else {
                    encode(half, false)
                };
                let opcode =
                    (bases[op as usize] & !0x0040f02f) | (vd << 12) | (d << 22) | vm | (m << 5);
                write_volatile(code as *mut u16, (opcode >> 16) as u16);
                write_volatile((code + 2) as *mut u16, opcode as u16);
                asm!("dsb", "isb", options(nostack));
                let mut actual = [0u32; 32];
                let flags: u32;
                asm!("vldmia {input}, {{s0-s31}}","vmsr fpscr, {control}","blx {code}",
                    "vstmia {output}, {{s0-s31}}","vmrs {flags}, fpscr",
                    input=in(reg)before.as_ptr(),output=in(reg)actual.as_mut_ptr(),control=in(reg)control,
                    code=in(reg)(code+1),flags=out(reg)flags,out("lr")_,
                    out("d0")_,out("d1")_,out("d2")_,out("d3")_,out("d4")_,out("d5")_,out("d6")_,out("d7")_,
                    out("d8")_,out("d9")_,out("d10")_,out("d11")_,out("d12")_,out("d13")_,out("d14")_,out("d15")_,options(nostack));
                let bad = actual
                    .iter()
                    .zip(expected)
                    .position(|(a, b)| *a != b)
                    .or_else(|| (flags != result[2]).then_some(32));
                if let Some(index) = bad {
                    write_volatile(RESULTS, 0xbad0f016);
                    write_volatile(RESULTS.add(27), count);
                    write_volatile(RESULTS.add(28), opcode);
                    write_volatile(RESULTS.add(29), index as u32);
                    write_volatile(
                        RESULTS.add(30),
                        if index == 32 { flags } else { actual[index] },
                    );
                    return false;
                }
                for word in actual.into_iter().chain([flags]) {
                    for byte in word.to_le_bytes() {
                        hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
                    }
                }
                count += 1;
                write_volatile(RESULTS.add(27), count);
            }
        }
    }
    write_volatile(RESULTS.add(28), hash as u32);
    write_volatile(RESULTS.add(29), (hash >> 32) as u32);
    write_volatile(RESULTS.add(30), 1);
    true
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn probe_main() -> ! {
    for i in 0..32 {
        write_volatile(RESULTS.add(i), 0);
    }
    write_volatile(0xe000ed88 as *mut u32, 0x00f00000);
    asm!("dsb", "isb", options(nostack));
    for (i, (op, bits, control, _)) in CASES.into_iter().enumerate() {
        let (lo, hi, flags) = execute(op, bits, control);
        write_volatile(RESULTS.add(1 + 3 * i), lo);
        write_volatile(RESULTS.add(2 + 3 * i), hi);
        write_volatile(RESULTS.add(3 + 3 * i), flags);
    }
    let matrix = register_matrix();
    write_volatile(RESULTS.add(25), read_volatile(0xe000ed28 as *const u32));
    write_volatile(RESULTS.add(26), read_volatile(0xe000ed2c as *const u32));
    if matrix {
        write_volatile(RESULTS, 0xab00f016);
    }
    loop {
        asm!("wfi");
    }
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
