//! Independent executed binary arithmetic under each FPSCR rounding mode.
use core::arch::asm;

fn emit(precision: u32, a: u64, b: u64, op: u32, control: u32, result: (u32, u32, u32)) {
    let words = [
        precision,
        a as u32,
        (a >> 32) as u32,
        b as u32,
        (b >> 32) as u32,
        op,
        control,
        result.0,
        result.1,
        result.2,
        0x4854_5241,
    ];
    let hex = b"0123456789abcdef";
    let mut line = [0u8; 90];
    for (i, byte) in words.into_iter().flat_map(u32::to_le_bytes).enumerate() {
        line[2 * i] = hex[(byte >> 4) as usize];
        line[2 * i + 1] = hex[(byte & 15) as usize];
    }
    line[88] = b'\n';
    unsafe {
        super::call(4, line.as_ptr() as u32);
    }
}

pub(super) fn run() {
    let singles = [
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
    let doubles = [
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
    let mut seed = 0xb719_41ce_d2a4_031du64;
    for i in 0..384 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let a32 = if i < 256 {
            singles[i / 16]
        } else {
            seed as u32
        };
        let a64 = if i < 256 { doubles[i / 16] } else { seed };
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let b32 = if i < 256 {
            singles[i % 16]
        } else {
            seed as u32
        };
        let b64 = if i < 256 { doubles[i % 16] } else { seed };
        for rounding in 0..4u32 {
            for mode in [0, 1 << 24, 1 << 25, 3 << 24] {
                let control =
                    0xa000_0000 | (rounding << 22) | mode | if i & 1 == 0 { 0 } else { 0x9f };
                for (op, (value, status)) in [
                    controlled!("vadd.f32 s0,s1,s2", a32, b32, control),
                    controlled!("vsub.f32 s0,s1,s2", a32, b32, control),
                    controlled!("vmul.f32 s0,s1,s2", a32, b32, control),
                    controlled!("vdiv.f32 s0,s1,s2", a32, b32, control),
                    controlled!("vsqrt.f32 s0,s1", a32, b32, control),
                ]
                .into_iter()
                .enumerate()
                {
                    emit(
                        32,
                        a32 as u64,
                        b32 as u64,
                        op as u32,
                        control,
                        (value, 0, status),
                    );
                }
                for (op, result) in [
                    controlled64!("vadd.f64 d0,d1,d2", a64, b64, control),
                    controlled64!("vsub.f64 d0,d1,d2", a64, b64, control),
                    controlled64!("vmul.f64 d0,d1,d2", a64, b64, control),
                    controlled64!("vdiv.f64 d0,d1,d2", a64, b64, control),
                    controlled64!("vsqrt.f64 d0,d1", a64, b64, control),
                ]
                .into_iter()
                .enumerate()
                {
                    emit(64, a64, b64, op as u32, control, result);
                }
            }
        }
    }
}
