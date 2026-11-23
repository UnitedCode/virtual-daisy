//! Executed FPv5 rounding/conversion oracle; no emulator code is linked here.
use core::arch::asm;

macro_rules! converted64 {
    ($op:literal, $a:expr, $control:expr) => {{
        let value: u32;
        let flags: u32;
        unsafe {
            asm!(
                "vmov d1, {al}, {ah}", "vmsr fpscr, {control}", $op,
                "vmov {value}, s0", "vmrs {flags}, fpscr",
                al = in(reg) $a as u32, ah = in(reg) ($a >> 32) as u32,
                control = in(reg) $control, value = out(reg) value,
                flags = out(reg) flags, out("s0") _, out("d1") _, options(nostack)
            );
        }
        (value, 0, flags)
    }};
}

fn emit(precision: u32, input: u64, op: u32, control: u32, result: (u32, u32, u32)) {
    let record = [
        0x544e_5256,
        precision,
        input as u32,
        (input >> 32) as u32,
        op,
        control,
        result.0,
        result.1,
        result.2,
    ];
    let hex = b"0123456789abcdef";
    let mut line = [0u8; 74];
    let mut n = 0;
    for word in record {
        for byte in word.to_le_bytes() {
            line[n] = hex[(byte >> 4) as usize];
            line[n + 1] = hex[(byte & 15) as usize];
            n += 2;
        }
    }
    line[72] = b'\n';
    unsafe {
        super::call(4, line.as_ptr() as u32);
    }
}

pub(super) fn run() {
    let special32 = [
        0,
        0x8000_0000,
        0x3f00_0000,
        0xbf00_0000,
        0x3fc0_0000,
        0xbfc0_0000,
        0x4020_0000,
        0xc020_0000,
        0x3eff_ffff,
        0x3f00_0001,
        0xbeff_ffff,
        0xbf00_0001,
        0x4eff_ffff,
        0x4f00_0000,
        0xcf00_0000,
        0xcf00_0001,
        0x4f7f_ffff,
        0x4f80_0000,
        0x4b00_0000,
        0xcb00_0000,
        0x7f7f_ffff,
        0xff7f_ffff,
        0x7f80_0000,
        0xff80_0000,
        1,
        0x8000_0001,
        0x007f_ffff,
        0x0080_0000,
        0x7fc1_2345,
        0xffc5_4321,
        0x7f81_2345,
        0xff85_4321,
    ];
    let special64 = [
        0,
        0x8000_0000_0000_0000,
        0.5f64.to_bits(),
        (-0.5f64).to_bits(),
        1.5f64.to_bits(),
        (-1.5f64).to_bits(),
        2.5f64.to_bits(),
        (-2.5f64).to_bits(),
        0x3fdf_ffff_ffff_ffff,
        0x3fe0_0000_0000_0001,
        0xbfdf_ffff_ffff_ffff,
        0xbfe0_0000_0000_0001,
        2_147_483_647.5f64.to_bits(),
        2_147_483_648.0f64.to_bits(),
        (-2_147_483_648.0f64).to_bits(),
        (-2_147_483_648.5f64).to_bits(),
        4_294_967_295.5f64.to_bits(),
        4_294_967_296.0f64.to_bits(),
        0x4330_0000_0000_0000,
        0xc330_0000_0000_0000,
        0x7fef_ffff_ffff_ffff,
        0xffef_ffff_ffff_ffff,
        0x7ff0_0000_0000_0000,
        0xfff0_0000_0000_0000,
        1,
        0x8000_0000_0000_0001,
        0x000f_ffff_ffff_ffff,
        0x0010_0000_0000_0000,
        0x7ff8_1234_5678_9abc,
        0xfff8_abcd_ef12_3456,
        0x7ff0_1234_5678_9abc,
        0xfff0_abcd_ef12_3456,
    ];
    let mut seed = 0x7f92_b342_15ab_781cu64;
    for i in 0..160 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let a32 = special32.get(i).copied().unwrap_or(seed as u32);
        let a64 = special64.get(i).copied().unwrap_or(seed);
        for mode in [0u32, 1 << 24, 1 << 25, 3 << 24] {
            let sticky = if i & 1 != 0 { 0x9f } else { 0 };
            let control = mode | 0xa000_0000 | (((i as u32) & 3) << 22) | sticky;
            let singles = [
                controlled!("vrinta.f32 s0,s1", a32, 0u32, control),
                controlled!("vrintn.f32 s0,s1", a32, 0u32, control),
                controlled!("vrintp.f32 s0,s1", a32, 0u32, control),
                controlled!("vrintm.f32 s0,s1", a32, 0u32, control),
                controlled!("vrintz.f32 s0,s1", a32, 0u32, control),
            ];
            let doubles = [
                controlled64!("vrinta.f64 d0,d1", a64, 0u64, control),
                controlled64!("vrintn.f64 d0,d1", a64, 0u64, control),
                controlled64!("vrintp.f64 d0,d1", a64, 0u64, control),
                controlled64!("vrintm.f64 d0,d1", a64, 0u64, control),
                controlled64!("vrintz.f64 d0,d1", a64, 0u64, control),
            ];
            for (op, (value, flags)) in singles.into_iter().enumerate() {
                emit(32, a32 as u64, op as u32, control, (value, 0, flags));
            }
            for (op, value) in doubles.into_iter().enumerate() {
                emit(64, a64, op as u32, control, value);
            }
            for rounding in 0..4 {
                let control = mode | 0xa000_0000 | (rounding << 22) | sticky;
                for (op, (value, flags)) in [
                    controlled!("vrintr.f32 s0,s1", a32, 0u32, control),
                    controlled!("vrintx.f32 s0,s1", a32, 0u32, control),
                ]
                .into_iter()
                .enumerate()
                {
                    emit(32, a32 as u64, op as u32 + 5, control, (value, 0, flags));
                }
                for (op, value) in [
                    controlled64!("vrintr.f64 d0,d1", a64, 0u64, control),
                    controlled64!("vrintx.f64 d0,d1", a64, 0u64, control),
                ]
                .into_iter()
                .enumerate()
                {
                    emit(64, a64, op as u32 + 5, control, value);
                }
                for (op, (value, flags)) in [
                    controlled!("vcvtr.s32.f32 s0,s1", a32, 0u32, control),
                    controlled!("vcvtr.u32.f32 s0,s1", a32, 0u32, control),
                ]
                .into_iter()
                .enumerate()
                {
                    emit(32, a32 as u64, op as u32 + 17, control, (value, 0, flags));
                }
                for (op, value) in [
                    converted64!("vcvtr.s32.f64 s0,d1", a64, control),
                    converted64!("vcvtr.u32.f64 s0,d1", a64, control),
                ]
                .into_iter()
                .enumerate()
                {
                    emit(64, a64, op as u32 + 17, control, value);
                }
            }
            let converted32 = [
                controlled!("vcvta.s32.f32 s0,s1", a32, 0u32, control),
                controlled!("vcvta.u32.f32 s0,s1", a32, 0u32, control),
                controlled!("vcvtn.s32.f32 s0,s1", a32, 0u32, control),
                controlled!("vcvtn.u32.f32 s0,s1", a32, 0u32, control),
                controlled!("vcvtp.s32.f32 s0,s1", a32, 0u32, control),
                controlled!("vcvtp.u32.f32 s0,s1", a32, 0u32, control),
                controlled!("vcvtm.s32.f32 s0,s1", a32, 0u32, control),
                controlled!("vcvtm.u32.f32 s0,s1", a32, 0u32, control),
            ];
            let converted64 = [
                converted64!("vcvta.s32.f64 s0,d1", a64, control),
                converted64!("vcvta.u32.f64 s0,d1", a64, control),
                converted64!("vcvtn.s32.f64 s0,d1", a64, control),
                converted64!("vcvtn.u32.f64 s0,d1", a64, control),
                converted64!("vcvtp.s32.f64 s0,d1", a64, control),
                converted64!("vcvtp.u32.f64 s0,d1", a64, control),
                converted64!("vcvtm.s32.f64 s0,d1", a64, control),
                converted64!("vcvtm.u32.f64 s0,d1", a64, control),
            ];
            for (op, (value, flags)) in converted32.into_iter().enumerate() {
                emit(32, a32 as u64, op as u32 + 7, control, (value, 0, flags));
            }
            for (op, value) in converted64.into_iter().enumerate() {
                emit(64, a64, op as u32 + 7, control, value);
            }
            for (op, (value, flags)) in [
                controlled!("vcvt.s32.f32 s0,s1", a32, 0u32, control),
                controlled!("vcvt.u32.f32 s0,s1", a32, 0u32, control),
            ]
            .into_iter()
            .enumerate()
            {
                emit(32, a32 as u64, op as u32 + 15, control, (value, 0, flags));
            }
            for (op, value) in [
                converted64!("vcvt.s32.f64 s0,d1", a64, control),
                converted64!("vcvt.u32.f64 s0,d1", a64, control),
            ]
            .into_iter()
            .enumerate()
            {
                emit(64, a64, op as u32 + 15, control, value);
            }
        }
    }
}
