//! Storage-format half conversions executed independently by ARM QEMU.
use core::arch::asm;

macro_rules! single {
    ($instruction:literal,$bits:expr,$control:expr) => {{
        let value:u32;let flags:u32;
        unsafe {asm!("vmov s2, {input}","vmov s0, {canary}","vmsr fpscr, {control}",
            $instruction,"vmov {value}, s0","vmrs {flags}, fpscr",
            input=in(reg)$bits as u32,canary=in(reg)0xa5a55a5au32,control=in(reg)$control,
            value=out(reg)value,flags=out(reg)flags,out("s0")_,out("s2")_,options(nostack));}
        (value,0,flags)
    }};
}
macro_rules! widen {
    ($instruction:literal,$bits:expr,$control:expr) => {{
        let lo:u32;let hi:u32;let flags:u32;
        unsafe {asm!("vmov s2, {input}","vmsr fpscr, {control}",$instruction,
            "vmov {lo}, {hi}, d0","vmrs {flags}, fpscr",
            input=in(reg)$bits as u32,control=in(reg)$control,
            lo=out(reg)lo,hi=out(reg)hi,flags=out(reg)flags,out("d0")_,out("s2")_,options(nostack));}
        (lo,hi,flags)
    }};
}
macro_rules! narrow {
    ($instruction:literal,$bits:expr,$control:expr) => {{
        let value:u32;let flags:u32;
        unsafe {asm!("vmov d1, {il}, {ih}","vmov s0, {canary}","vmsr fpscr, {control}",
            $instruction,"vmov {value}, s0","vmrs {flags}, fpscr",
            il=in(reg)$bits as u32,ih=in(reg)($bits>>32) as u32,
            canary=in(reg)0xa5a55a5au32,control=in(reg)$control,
            value=out(reg)value,flags=out(reg)flags,out("s0")_,out("d1")_,options(nostack));}
        (value,0,flags)
    }};
}
fn execute(op: u32, bits: u64, control: u32) -> (u32, u32, u32) {
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
fn emit(op: u32, bits: u64, control: u32) {
    let (lo, hi, flags) = execute(op, bits, control);
    let words = [
        op,
        bits as u32,
        (bits >> 32) as u32,
        0xa5a55a5a,
        control,
        lo,
        hi,
        flags,
        0x464c4148,
    ];
    let hex = b"0123456789abcdef";
    let mut line = [0u8; 74];
    for (i, byte) in words.into_iter().flat_map(u32::to_le_bytes).enumerate() {
        line[2 * i] = hex[(byte >> 4) as usize];
        line[2 * i + 1] = hex[(byte & 15) as usize];
    }
    line[72] = b'\n';
    unsafe {
        super::call(4, line.as_ptr() as u32);
    }
}
fn packed(half: u32, top: u32) -> u64 {
    u64::from(if top == 0 {
        0x5a5a0000 | half
    } else {
        half << 16 | 0xa5a5
    })
}
pub(super) fn run() {
    // Exhaust every half encoding in IEEE and alternative formats, in both
    // destination precisions. Separate sweeps cover controls and both selectors.
    for half in 0..=65535u32 {
        let top = half & 1;
        for alternative in 0..2 {
            for double in 0..2 {
                emit(
                    double * 4 + top,
                    packed(half, top),
                    0xa0000000 | alternative << 26,
                );
            }
        }
    }
    let halves = [
        0u32, 0x8000, 1, 0x8001, 0x03ff, 0x83ff, 0x0400, 0x8400, 0x3c00, 0xbc00, 0x7bff, 0xfbff,
        0x7c00, 0xfc00, 0x7d01, 0xfd01, 0x7e01, 0xfe01, 0x7fff, 0xffff, 0x0401, 0x8401, 0x3555,
        0xb555, 2, 0x8002, 0x3c01, 0xbc01, 0x4000, 0xc000, 0x7800, 0xf800,
    ];
    let singles = [
        0u32, 0x80000000, 0x3f800000, 0xbf800000, 0x00800000, 0x7f7fffff, 1, 0x80000001,
        0x7f800000, 0xff800000, 0x7fc12345, 0x7f812345, 0xffc12345, 0xff812345, 0x38800000,
        0x387fffff, 0x477fe000, 0x477ff000, 0x47800000, 0x47ffe000, 0x47fff000, 0x48000000,
        0x33800000, 0x33000000, 0x33000001, 0x32ffffff, 0x3f801000, 0x3f801001, 0xbf801000,
        0xb3000001, 0xb3000000, 0x38800001,
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
        0x3f10000000000000,
        0x3f0fffffffffffff,
        0x40effc0000000000,
        0x40effe0000000000,
        0x40f0000000000000,
        0x40fffc0000000000,
        0x40fffe0000000000,
        0x4100000000000000,
        0x3e70000000000000,
        0x3e60000000000000,
        0x3e60000000000001,
        0x3e5fffffffffffff,
        0x3ff0020000000000,
        0x3ff0020000000001,
        0xbff0020000000000,
        0xbe60000000000001,
        0xbe60000000000000,
        0x3f10000000000001,
    ];
    let mut seed = 0x63a52cd817f490ebu64;
    for i in 0..512 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let random32 = if i % 4 == 0 {
            seed as u32
        } else {
            ((seed as u32) & 0x807fffff) | (((seed >> 32) % 52 + 97) as u32) << 23
        };
        let random64 = if i % 4 == 0 {
            seed
        } else {
            (seed & 0x800fffffffffffff) | ((seed >> 52) % 52 + 993) << 52
        };
        let a = singles.get(i).copied().unwrap_or(random32);
        let b = doubles.get(i).copied().unwrap_or(random64);
        for mode in 0..32 {
            let control = 0xa0000000
                | (mode & 3) << 22
                | ((mode >> 2) & 1) << 24
                | ((mode >> 3) & 1) << 25
                | ((mode >> 4) & 1) << 26;
            for top in 0..2 {
                emit(2 + top, u64::from(a), control);
                emit(6 + top, b, control);
                if let Some(&half) = halves.get(i) {
                    emit(top, packed(half, top), control);
                    emit(4 + top, packed(half, top), control);
                }
            }
        }
    }
}
