//! Executed fused operations; no emulator implementation is linked here.
use core::arch::asm;

macro_rules! fused32 {
    ($op:literal, $a:expr, $b:expr, $c:expr, $control:expr) => {{
        let value: u32; let flags: u32;
        unsafe { asm!("vmov s0, {c}", "vmov s1, {a}", "vmov s2, {b}",
            "vmsr fpscr, {control}", $op, "vmov {value}, s0", "vmrs {flags}, fpscr",
            a=in(reg)$a, b=in(reg)$b, c=in(reg)$c, control=in(reg)$control,
            value=out(reg)value, flags=out(reg)flags,
            out("s0")_, out("s1")_, out("s2")_, options(nostack)); }
        (value, 0, flags)
    }};
}
macro_rules! fused64 {
    ($op:literal, $a:expr, $b:expr, $c:expr, $control:expr) => {{
        let lo: u32; let hi: u32; let flags: u32;
        unsafe { asm!("vmov d0, {cl}, {ch}", "vmov d1, {al}, {ah}", "vmov d2, {bl}, {bh}",
            "vmsr fpscr, {control}", $op, "vmov {lo}, {hi}, d0", "vmrs {flags}, fpscr",
            al=in(reg)$a as u32, ah=in(reg)($a >> 32) as u32,
            bl=in(reg)$b as u32, bh=in(reg)($b >> 32) as u32,
            cl=in(reg)$c as u32, ch=in(reg)($c >> 32) as u32, control=in(reg)$control,
            lo=out(reg)lo, hi=out(reg)hi, flags=out(reg)flags,
            out("d0")_, out("d1")_, out("d2")_, options(nostack)); }
        (lo, hi, flags)
    }};
}
fn emit(precision: u32, a: u64, b: u64, c: u64, op: u32, control: u32, result: (u32, u32, u32)) {
    let words = [
        precision,
        a as u32,
        (a >> 32) as u32,
        b as u32,
        (b >> 32) as u32,
        c as u32,
        (c >> 32) as u32,
        op,
        control,
        result.0,
        result.1,
        result.2,
        0x4553_5546,
    ];
    let hex = b"0123456789abcdef";
    let mut line = [0u8; 106];
    for (i, byte) in words.into_iter().flat_map(u32::to_le_bytes).enumerate() {
        line[i * 2] = hex[(byte >> 4) as usize];
        line[i * 2 + 1] = hex[(byte & 15) as usize];
    }
    line[104] = b'\n';
    unsafe {
        super::call(4, line.as_ptr() as u32);
    }
}
pub(super) fn run() {
    let s = [
        0u32,
        0x8000_0000,
        0x3f80_0000,
        0xbf80_0000,
        0x0080_0000,
        0x7f7f_ffff,
        1,
        0x8000_0001,
        0x7f80_0000,
        0xff80_0000,
        0x7fc1_2345,
        0x7f81_2345,
        0x3f00_0000,
        0x3380_0000,
        0x0080_0001,
        0x3f80_0001,
    ];
    let d = [
        0u64,
        0x8000_0000_0000_0000,
        0x3ff0_0000_0000_0000,
        0xbff0_0000_0000_0000,
        0x0010_0000_0000_0000,
        0x7fef_ffff_ffff_ffff,
        1,
        0x8000_0000_0000_0001,
        0x7ff0_0000_0000_0000,
        0xfff0_0000_0000_0000,
        0x7ff8_1234_5678_9abc,
        0x7ff0_1234_5678_9abc,
        0x3fe0_0000_0000_0000,
        0x3ca0_0000_0000_0000,
        0x0010_0000_0000_0001,
        0x3ff0_0000_0000_0001,
    ];
    let mut seed = 0xa935_c170_21b4_2d83u64;
    for i in 0..384 {
        let mut random = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let ra = random();
        let rb = random();
        let rc = random();
        let (mut a32, mut b32, mut c32, mut a64, mut b64, mut c64) = if i < 256 {
            (
                s[i / 16],
                s[i % 16],
                s[(i / 16 + i % 16 * 3) % 16],
                d[i / 16],
                d[i % 16],
                d[(i / 16 + i % 16 * 3) % 16],
            )
        } else {
            (ra as u32, rb as u32, rc as u32, ra, rb, rc)
        };
        match i {
            256 => {
                a32 = 1;
                b32 = 0x8000_0001;
                c32 = 0x0080_0000;
                a64 = 1;
                b64 = 0x8000_0000_0000_0001;
                c64 = 0x0010_0000_0000_0000;
            }
            257 => {
                a32 = 0x7f7f_ffff;
                b32 = 2f32.to_bits();
                c32 = 0xff7f_ffff;
                a64 = 0x7fef_ffff_ffff_ffff;
                b64 = 2f64.to_bits();
                c64 = 0xffef_ffff_ffff_ffff;
            }
            258 => {
                a32 = 0x3f80_0001;
                b32 = 0x3f7f_fffe;
                c32 = (-1f32).to_bits();
                a64 = 0x3ff0_0000_0000_0001;
                b64 = 0x3fef_ffff_ffff_fffe;
                c64 = (-1f64).to_bits();
            }
            260 => {
                a32 = 1f32.to_bits();
                b32 = a32;
                c32 = a32;
                a64 = 1f64.to_bits();
                b64 = a64;
                c64 = a64;
            }
            264 => {
                a32 = 0x0080_0000;
                b32 = 0x8080_0000;
                c32 = 0x0080_0000;
                a64 = 0x0010_0000_0000_0000;
                b64 = 0x8010_0000_0000_0000;
                c64 = 0x0010_0000_0000_0000;
            }
            _ => {}
        }
        for rounding in 0..4u32 {
            for mode in [0, 1 << 24, 1 << 25, 3 << 24] {
                let control = 0xa000_0000 | (rounding << 22) | mode;
                for (op, result) in [
                    fused32!("vfma.f32 s0,s1,s2", a32, b32, c32, control),
                    fused32!("vfms.f32 s0,s1,s2", a32, b32, c32, control),
                    fused32!("vfnms.f32 s0,s1,s2", a32, b32, c32, control),
                    fused32!("vfnma.f32 s0,s1,s2", a32, b32, c32, control),
                ]
                .into_iter()
                .enumerate()
                {
                    emit(
                        32, a32 as u64, b32 as u64, c32 as u64, op as u32, control, result,
                    );
                }
                for (op, result) in [
                    fused64!("vfma.f64 d0,d1,d2", a64, b64, c64, control),
                    fused64!("vfms.f64 d0,d1,d2", a64, b64, c64, control),
                    fused64!("vfnms.f64 d0,d1,d2", a64, b64, c64, control),
                    fused64!("vfnma.f64 d0,d1,d2", a64, b64, c64, control),
                ]
                .into_iter()
                .enumerate()
                {
                    emit(64, a64, b64, c64, op as u32, control, result);
                }
            }
        }
    }
}
