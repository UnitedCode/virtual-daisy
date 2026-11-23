//! Saturating word arithmetic executed by QEMU's ARM assembler/CPU.
use core::arch::asm;
macro_rules! execute {
    ($instruction:literal,$a:expr,$b:expr,$control:expr)=>{{
        let result:u32;let flags:u32;
        unsafe {asm!("msr APSR_nzcvqg, {control}",$instruction,"mrs {flags}, APSR",
            in("r1")$a,in("r2")$b,lateout("r0")result,control=in(reg)$control,
            flags=out(reg)flags,options(nostack));}
        (result,flags)
    }};
}
fn emit(op: u32, a: u32, b: u32, control: u32) {
    let (result, flags) = match op {
        0 => execute!("qadd r0, r1, r2", a, b, control),
        1 => execute!("qsub r0, r1, r2", a, b, control),
        2 => execute!("qdadd r0, r1, r2", a, b, control),
        _ => execute!("qdsub r0, r1, r2", a, b, control),
    };
    let words = [op, a, b, control, result, flags, 0x44444151];
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
    let values = [
        0, 1, 0xffffffff, 0x7fffffff, 0x80000000, 0x40000000, 0xc0000000, 0x3fffffff, 0x40000001,
        0xbfffffff, 0xc0000001, 0x7ffffffe, 0x80000001, 0x00008000, 0xffff8000, 0x20000000,
    ];
    let mut seed = 0x2597b4c1u32;
    for i in 0..512 {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let a = if i < 256 { values[i / 16] } else { seed };
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let b = if i < 256 { values[i % 16] } else { seed };
        for op in 0..4 {
            for control in [0xa00a0000, 0xa80a0000, 0x50050000, 0x58050000] {
                emit(op, a, b, control);
            }
        }
    }
}
