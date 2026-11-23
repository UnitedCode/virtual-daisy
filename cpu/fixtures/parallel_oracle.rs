//! Independent QEMU parallel-DSP arithmetic and APSR records.
use core::arch::asm;
macro_rules! execute { ($instruction:literal,$a:expr,$b:expr,$control:expr)=>{{ let result:u32;let flags:u32;let opcode:u32; unsafe {asm!("msr APSR_nzcvqg, {control}","adr {ptr}, 2f","ldr {opcode}, [{ptr}]","ror {opcode}, {opcode}, #16","2:",$instruction,"mrs {flags}, APSR",in("r1")$a,in("r2")$b,lateout("r0")result,control=in(reg)$control,ptr=out(reg)_,opcode=out(reg)opcode,flags=out(reg)flags,options(nostack));} (opcode,result,flags) }}; }
fn emit(op: u32, a: u32, b: u32, control: u32) {
    let (opcode, result, flags) = match op {
        0 => execute!("sadd16 r0,r1,r2", a, b, control),
        1 => execute!("sasx r0,r1,r2", a, b, control),
        2 => execute!("ssax r0,r1,r2", a, b, control),
        3 => execute!("ssub16 r0,r1,r2", a, b, control),
        4 => execute!("sadd8 r0,r1,r2", a, b, control),
        5 => execute!("ssub8 r0,r1,r2", a, b, control),
        6 => execute!("qadd16 r0,r1,r2", a, b, control),
        7 => execute!("qasx r0,r1,r2", a, b, control),
        8 => execute!("qsax r0,r1,r2", a, b, control),
        9 => execute!("qsub16 r0,r1,r2", a, b, control),
        10 => execute!("qadd8 r0,r1,r2", a, b, control),
        11 => execute!("qsub8 r0,r1,r2", a, b, control),
        12 => execute!("shadd16 r0,r1,r2", a, b, control),
        13 => execute!("shasx r0,r1,r2", a, b, control),
        14 => execute!("shsax r0,r1,r2", a, b, control),
        15 => execute!("shsub16 r0,r1,r2", a, b, control),
        16 => execute!("shadd8 r0,r1,r2", a, b, control),
        17 => execute!("shsub8 r0,r1,r2", a, b, control),
        18 => execute!("uadd16 r0,r1,r2", a, b, control),
        19 => execute!("uasx r0,r1,r2", a, b, control),
        20 => execute!("usax r0,r1,r2", a, b, control),
        21 => execute!("usub16 r0,r1,r2", a, b, control),
        22 => execute!("uadd8 r0,r1,r2", a, b, control),
        23 => execute!("usub8 r0,r1,r2", a, b, control),
        24 => execute!("uqadd16 r0,r1,r2", a, b, control),
        25 => execute!("uqasx r0,r1,r2", a, b, control),
        26 => execute!("uqsax r0,r1,r2", a, b, control),
        27 => execute!("uqsub16 r0,r1,r2", a, b, control),
        28 => execute!("uqadd8 r0,r1,r2", a, b, control),
        29 => execute!("uqsub8 r0,r1,r2", a, b, control),
        30 => execute!("uhadd16 r0,r1,r2", a, b, control),
        31 => execute!("uhasx r0,r1,r2", a, b, control),
        32 => execute!("uhsax r0,r1,r2", a, b, control),
        33 => execute!("uhsub16 r0,r1,r2", a, b, control),
        34 => execute!("uhadd8 r0,r1,r2", a, b, control),
        35 => execute!("uhsub8 r0,r1,r2", a, b, control),
        _ => unreachable!(),
    };
    let words = [opcode, a, b, control, result, flags, 0x504d4953];
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
        0u32, 1, 0xffffffff, 0x7fff7fff, 0x80008000, 0x7f7f7f7f, 0x80808080, 0x00ff00ff,
        0xff00ff00, 0x0000ffff, 0xffff0000, 0x7fff8000, 0x80007fff, 0x01010101, 0xfefefefe,
        0x5555aaaa,
    ];
    let mut seed = 0x71b6c9d3u32;
    for i in 0..512 {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let a = if i < 256 { values[i / 16] } else { seed };
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let b = if i < 256 { values[i % 16] } else { seed };
        for op in 0..36 {
            for control in [0xa00a0000, 0xa80a0000, 0x50050000, 0x58050000] {
                emit(op, a, b, control);
            }
        }
    }
}
