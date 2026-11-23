//! Precision conversions executed by QEMU, without emulator implementation code.
use core::arch::asm;

fn execute(direction: u32, bits: u64, control: u32) -> (u32, u32, u32) {
    let lo: u32;
    let hi: u32;
    let flags: u32;
    unsafe {
        if direction == 0 {
            asm!("vmov s2, {input}", "vmsr fpscr, {control}",
                "vcvt.f64.f32 d0, s2", "vmov {lo}, {hi}, d0", "vmrs {flags}, fpscr",
                input=in(reg)bits as u32, control=in(reg)control,
                lo=out(reg)lo, hi=out(reg)hi, flags=out(reg)flags,
                out("d0")_, out("s2")_, options(nostack));
        } else {
            asm!("vmov d1, {il}, {ih}", "vmsr fpscr, {control}",
                "vcvt.f32.f64 s0, d1", "vmov {lo}, s0", "vmrs {flags}, fpscr",
                il=in(reg)bits as u32, ih=in(reg)(bits>>32) as u32, control=in(reg)control,
                lo=out(reg)lo, flags=out(reg)flags,
                out("s0")_, out("d1")_, options(nostack));
            hi = 0;
        }
    }
    (lo, hi, flags)
}
fn emit(direction: u32, bits: u64, control: u32, result: (u32, u32, u32)) {
    let words = [
        direction,
        bits as u32,
        (bits >> 32) as u32,
        control,
        result.0,
        result.1,
        result.2,
    ];
    let hex = b"0123456789abcdef";
    let mut line = [0u8; 58];
    for (i, byte) in words.into_iter().flat_map(u32::to_le_bytes).enumerate() {
        line[2 * i] = hex[(byte >> 4) as usize];
        line[2 * i + 1] = hex[(byte & 15) as usize];
    }
    line[56] = b'\n';
    unsafe {
        super::call(4, line.as_ptr() as u32);
    }
}
pub(super) fn run() {
    let singles = [
        0u32, 0x80000000, 0x3f800000, 0xbf800000, 0x00800000, 0x7f7fffff, 1, 0x80000001,
        0x7f800000, 0xff800000, 0x7fc12345, 0x7f812345, 0xffc12345, 0xff812345, 0x007fffff,
        0x807fffff,
    ];
    let doubles = [
        0u64,
        0x8000000000000000,
        0x3ff0000000000000,
        0xbff0000000000000,
        0x0010000000000000,
        0x7fefffffffffffff,
        1,
        0x8000000000000001,
        0x7ff0000000000000,
        0xfff0000000000000,
        0x7ff8123456789abc,
        0x7ff0123456789abc,
        0xfff8123456789abc,
        0xfff0123456789abc,
        0x3810000000000000,
        0x380fffffffffffff,
        0x380fffffc0000000,
        0x380fffffe0000000,
        0x380fffffe0000001,
        0x47efffffe0000000,
        0x47effffff0000000,
        0x47f0000000000000,
        0x36a0000000000000,
        0x3690000000000000,
        0x3690000000000001,
        0x368fffffffffffff,
        0x3ff0000010000000,
        0x3ff0000010000001,
        0x3ff0000020000000,
        0xb80fffffffffffff,
        0xc7f0000000000000,
        0xb690000000000000,
    ];
    let mut seed = 0xb4a17c8e9350d2f6u64;
    for i in 0..1024 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let a = singles.get(i).copied().unwrap_or(seed as u32);
        let b = doubles.get(i).copied().unwrap_or(seed);
        for rounding in 0..4 {
            for mode in 0..4 {
                let control = 0xa0000000 | rounding << 22 | (mode & 1) << 24 | (mode >> 1) << 25;
                emit(0, u64::from(a), control, execute(0, u64::from(a), control));
                emit(1, b, control, execute(1, b, control));
            }
        }
    }
}
