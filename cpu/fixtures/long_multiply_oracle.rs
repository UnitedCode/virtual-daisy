//! Independent QEMU long multiply/accumulate records.
use core::arch::asm;
macro_rules! execute {($instruction:literal,$a:expr,$b:expr,$acc:expr,$control:expr)=>{{let mut lo=$acc as u32;let mut hi=($acc>>32)as u32;let flags:u32;let opcode:u32;unsafe{asm!("msr APSR_nzcvqg, {control}","adr {ptr}, 2f","ldr {opcode}, [{ptr}]","ror {opcode}, {opcode}, #16","2:",$instruction,"mrs {flags}, APSR",in("r1")$a,in("r2")$b,inout("r0")lo,inout("r3")hi,control=in(reg)$control,ptr=out(reg)_,opcode=out(reg)opcode,flags=out(reg)flags,options(nostack));}(opcode,lo,hi,flags)}};}
fn emit(op: u32, a: u32, b: u32, acc: u64, control: u32) {
    let (opcode, lo, hi, flags) = match op {
        0 => execute!("umull r0,r3,r1,r2", a, b, acc, control),
        1 => execute!("umlal r0,r3,r1,r2", a, b, acc, control),
        2 => execute!("smull r0,r3,r1,r2", a, b, acc, control),
        3 => execute!("smlal r0,r3,r1,r2", a, b, acc, control),
        4 => execute!("umaal r0,r3,r1,r2", a, b, acc, control),
        5 => execute!("smlalbb r0,r3,r1,r2", a, b, acc, control),
        6 => execute!("smlalbt r0,r3,r1,r2", a, b, acc, control),
        7 => execute!("smlaltb r0,r3,r1,r2", a, b, acc, control),
        8 => execute!("smlaltt r0,r3,r1,r2", a, b, acc, control),
        9 => execute!("smlald r0,r3,r1,r2", a, b, acc, control),
        10 => execute!("smlaldx r0,r3,r1,r2", a, b, acc, control),
        11 => execute!("smlsld r0,r3,r1,r2", a, b, acc, control),
        12 => execute!("smlsldx r0,r3,r1,r2", a, b, acc, control),
        _ => unreachable!(),
    };
    let words = [
        opcode,
        a,
        b,
        acc as u32,
        (acc >> 32) as u32,
        control,
        lo,
        hi,
        flags,
        0x4d4c4c44,
    ];
    let hex = b"0123456789abcdef";
    let mut line = [0u8; 82];
    for (i, byte) in words.into_iter().flat_map(u32::to_le_bytes).enumerate() {
        line[2 * i] = hex[(byte >> 4) as usize];
        line[2 * i + 1] = hex[(byte & 15) as usize];
    }
    line[80] = b'\n';
    unsafe {
        super::call(4, line.as_ptr() as u32);
    }
}
pub(super) fn run() {
    let values = [
        0u32, 1, 0xffffffff, 0x7fffffff, 0x80000000, 0x7fff7fff, 0x80008000, 0x7fff8000,
        0x80007fff, 0x0000ffff, 0xffff0000, 0x00ff00ff, 0xff00ff00, 0x40000000, 0xc0000000,
        0x5555aaaa,
    ];
    let mut seed = 0xa1b6c9d3u32;
    for i in 0..512 {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let a = if i < 256 { values[i / 16] } else { seed };
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let b = if i < 256 { values[i % 16] } else { seed };
        for acc in [
            0u64,
            1,
            u64::MAX,
            0x000000007fffffff,
            0x0000000080000000,
            0x7fffffff00000000,
            0x7fffffff7fffffff,
            0x7fffffff80000000,
            0x8000000000000000,
            0x800000007fffffff,
            0x8000000080000000,
        ] {
            for op in 0..13 {
                for control in [0xa00a0000, 0xa80a0000, 0x50050000, 0x58050000] {
                    emit(op, a, b, acc, control);
                }
            }
        }
    }
}
