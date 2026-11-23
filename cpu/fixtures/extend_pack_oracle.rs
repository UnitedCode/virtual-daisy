//! Independent QEMU rotated extensions and halfword packing.
use core::arch::asm;
macro_rules! execute {($instruction:literal,$a:expr,$b:expr,$control:expr)=>{{let result:u32;let flags:u32;let opcode:u32;unsafe{asm!("msr APSR_nzcvqg, {control}","adr {ptr}, 2f","ldr {opcode}, [{ptr}]","ror {opcode}, {opcode}, #16","2:",$instruction,"mrs {flags}, APSR",in("r1")$a,in("r2")$b,lateout("r0")result,control=in(reg)$control,ptr=out(reg)_,opcode=out(reg)opcode,flags=out(reg)flags,options(nostack));}(opcode,result,flags)}};}
fn emit(op: u32, a: u32, b: u32, control: u32) {
    let (opcode, result, flags) = match op {
        0 => execute!("sxtab r0,r1,r2,ror #0", a, b, control),
        1 => execute!("sxtab r0,r1,r2,ror #8", a, b, control),
        2 => execute!("sxtab r0,r1,r2,ror #16", a, b, control),
        3 => execute!("sxtab r0,r1,r2,ror #24", a, b, control),
        4 => execute!("sxtah r0,r1,r2,ror #0", a, b, control),
        5 => execute!("sxtah r0,r1,r2,ror #8", a, b, control),
        6 => execute!("sxtah r0,r1,r2,ror #16", a, b, control),
        7 => execute!("sxtah r0,r1,r2,ror #24", a, b, control),
        8 => execute!("sxtab16 r0,r1,r2,ror #0", a, b, control),
        9 => execute!("sxtab16 r0,r1,r2,ror #8", a, b, control),
        10 => execute!("sxtab16 r0,r1,r2,ror #16", a, b, control),
        11 => execute!("sxtab16 r0,r1,r2,ror #24", a, b, control),
        12 => execute!("uxtab r0,r1,r2,ror #0", a, b, control),
        13 => execute!("uxtab r0,r1,r2,ror #8", a, b, control),
        14 => execute!("uxtab r0,r1,r2,ror #16", a, b, control),
        15 => execute!("uxtab r0,r1,r2,ror #24", a, b, control),
        16 => execute!("uxtah r0,r1,r2,ror #0", a, b, control),
        17 => execute!("uxtah r0,r1,r2,ror #8", a, b, control),
        18 => execute!("uxtah r0,r1,r2,ror #16", a, b, control),
        19 => execute!("uxtah r0,r1,r2,ror #24", a, b, control),
        20 => execute!("uxtab16 r0,r1,r2,ror #0", a, b, control),
        21 => execute!("uxtab16 r0,r1,r2,ror #8", a, b, control),
        22 => execute!("uxtab16 r0,r1,r2,ror #16", a, b, control),
        23 => execute!("uxtab16 r0,r1,r2,ror #24", a, b, control),
        24 => execute!("sxtb16 r0,r2,ror #0", a, b, control),
        25 => execute!("sxtb16 r0,r2,ror #8", a, b, control),
        26 => execute!("sxtb16 r0,r2,ror #16", a, b, control),
        27 => execute!("sxtb16 r0,r2,ror #24", a, b, control),
        28 => execute!("uxtb16 r0,r2,ror #0", a, b, control),
        29 => execute!("uxtb16 r0,r2,ror #8", a, b, control),
        30 => execute!("uxtb16 r0,r2,ror #16", a, b, control),
        31 => execute!("uxtb16 r0,r2,ror #24", a, b, control),
        32 => execute!("sxtb.w r0,r2,ror #0", a, b, control),
        33 => execute!("sxtb.w r0,r2,ror #8", a, b, control),
        34 => execute!("sxtb.w r0,r2,ror #16", a, b, control),
        35 => execute!("sxtb.w r0,r2,ror #24", a, b, control),
        36 => execute!("sxth.w r0,r2,ror #0", a, b, control),
        37 => execute!("sxth.w r0,r2,ror #8", a, b, control),
        38 => execute!("sxth.w r0,r2,ror #16", a, b, control),
        39 => execute!("sxth.w r0,r2,ror #24", a, b, control),
        40 => execute!("uxtb.w r0,r2,ror #0", a, b, control),
        41 => execute!("uxtb.w r0,r2,ror #8", a, b, control),
        42 => execute!("uxtb.w r0,r2,ror #16", a, b, control),
        43 => execute!("uxtb.w r0,r2,ror #24", a, b, control),
        44 => execute!("uxth.w r0,r2,ror #0", a, b, control),
        45 => execute!("uxth.w r0,r2,ror #8", a, b, control),
        46 => execute!("uxth.w r0,r2,ror #16", a, b, control),
        47 => execute!("uxth.w r0,r2,ror #24", a, b, control),
        48 => execute!("pkhbt r0,r1,r2,lsl #0", a, b, control),
        49 => execute!("pkhbt r0,r1,r2,lsl #1", a, b, control),
        50 => execute!("pkhbt r0,r1,r2,lsl #2", a, b, control),
        51 => execute!("pkhbt r0,r1,r2,lsl #3", a, b, control),
        52 => execute!("pkhbt r0,r1,r2,lsl #4", a, b, control),
        53 => execute!("pkhbt r0,r1,r2,lsl #5", a, b, control),
        54 => execute!("pkhbt r0,r1,r2,lsl #6", a, b, control),
        55 => execute!("pkhbt r0,r1,r2,lsl #7", a, b, control),
        56 => execute!("pkhbt r0,r1,r2,lsl #8", a, b, control),
        57 => execute!("pkhbt r0,r1,r2,lsl #9", a, b, control),
        58 => execute!("pkhbt r0,r1,r2,lsl #10", a, b, control),
        59 => execute!("pkhbt r0,r1,r2,lsl #11", a, b, control),
        60 => execute!("pkhbt r0,r1,r2,lsl #12", a, b, control),
        61 => execute!("pkhbt r0,r1,r2,lsl #13", a, b, control),
        62 => execute!("pkhbt r0,r1,r2,lsl #14", a, b, control),
        63 => execute!("pkhbt r0,r1,r2,lsl #15", a, b, control),
        64 => execute!("pkhbt r0,r1,r2,lsl #16", a, b, control),
        65 => execute!("pkhbt r0,r1,r2,lsl #17", a, b, control),
        66 => execute!("pkhbt r0,r1,r2,lsl #18", a, b, control),
        67 => execute!("pkhbt r0,r1,r2,lsl #19", a, b, control),
        68 => execute!("pkhbt r0,r1,r2,lsl #20", a, b, control),
        69 => execute!("pkhbt r0,r1,r2,lsl #21", a, b, control),
        70 => execute!("pkhbt r0,r1,r2,lsl #22", a, b, control),
        71 => execute!("pkhbt r0,r1,r2,lsl #23", a, b, control),
        72 => execute!("pkhbt r0,r1,r2,lsl #24", a, b, control),
        73 => execute!("pkhbt r0,r1,r2,lsl #25", a, b, control),
        74 => execute!("pkhbt r0,r1,r2,lsl #26", a, b, control),
        75 => execute!("pkhbt r0,r1,r2,lsl #27", a, b, control),
        76 => execute!("pkhbt r0,r1,r2,lsl #28", a, b, control),
        77 => execute!("pkhbt r0,r1,r2,lsl #29", a, b, control),
        78 => execute!("pkhbt r0,r1,r2,lsl #30", a, b, control),
        79 => execute!("pkhbt r0,r1,r2,lsl #31", a, b, control),
        80 => execute!("pkhtb r0,r1,r2,asr #1", a, b, control),
        81 => execute!("pkhtb r0,r1,r2,asr #2", a, b, control),
        82 => execute!("pkhtb r0,r1,r2,asr #3", a, b, control),
        83 => execute!("pkhtb r0,r1,r2,asr #4", a, b, control),
        84 => execute!("pkhtb r0,r1,r2,asr #5", a, b, control),
        85 => execute!("pkhtb r0,r1,r2,asr #6", a, b, control),
        86 => execute!("pkhtb r0,r1,r2,asr #7", a, b, control),
        87 => execute!("pkhtb r0,r1,r2,asr #8", a, b, control),
        88 => execute!("pkhtb r0,r1,r2,asr #9", a, b, control),
        89 => execute!("pkhtb r0,r1,r2,asr #10", a, b, control),
        90 => execute!("pkhtb r0,r1,r2,asr #11", a, b, control),
        91 => execute!("pkhtb r0,r1,r2,asr #12", a, b, control),
        92 => execute!("pkhtb r0,r1,r2,asr #13", a, b, control),
        93 => execute!("pkhtb r0,r1,r2,asr #14", a, b, control),
        94 => execute!("pkhtb r0,r1,r2,asr #15", a, b, control),
        95 => execute!("pkhtb r0,r1,r2,asr #16", a, b, control),
        96 => execute!("pkhtb r0,r1,r2,asr #17", a, b, control),
        97 => execute!("pkhtb r0,r1,r2,asr #18", a, b, control),
        98 => execute!("pkhtb r0,r1,r2,asr #19", a, b, control),
        99 => execute!("pkhtb r0,r1,r2,asr #20", a, b, control),
        100 => execute!("pkhtb r0,r1,r2,asr #21", a, b, control),
        101 => execute!("pkhtb r0,r1,r2,asr #22", a, b, control),
        102 => execute!("pkhtb r0,r1,r2,asr #23", a, b, control),
        103 => execute!("pkhtb r0,r1,r2,asr #24", a, b, control),
        104 => execute!("pkhtb r0,r1,r2,asr #25", a, b, control),
        105 => execute!("pkhtb r0,r1,r2,asr #26", a, b, control),
        106 => execute!("pkhtb r0,r1,r2,asr #27", a, b, control),
        107 => execute!("pkhtb r0,r1,r2,asr #28", a, b, control),
        108 => execute!("pkhtb r0,r1,r2,asr #29", a, b, control),
        109 => execute!("pkhtb r0,r1,r2,asr #30", a, b, control),
        110 => execute!("pkhtb r0,r1,r2,asr #31", a, b, control),
        111 => execute!("pkhtb r0,r1,r2,asr #32", a, b, control),
        _ => unreachable!(),
    };
    let words = [opcode, a, b, control, result, flags, 0x50545845];
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
        0u32, 1, 0xffffffff, 0x7fffffff, 0x80000000, 0x7fff7fff, 0x80008000, 0x7fff8000,
        0x80007fff, 0x0000ffff, 0xffff0000, 0x00ff00ff, 0xff00ff00, 0x40000000, 0xc0000000,
        0x5555aaaa,
    ];
    let mut seed = 0xb1b6c9d3u32;
    for i in 0..512 {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let a = if i < 256 { values[i / 16] } else { seed };
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let b = if i < 256 { values[i % 16] } else { seed };
        for op in 0..112 {
            for control in [0xa00a0000, 0xa80a0000, 0x50050000, 0x58050000] {
                emit(op, a, b, control);
            }
        }
    }
}
