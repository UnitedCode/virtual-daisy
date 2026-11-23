//! Independent QEMU DSP multiply and absolute-difference records.
use core::arch::asm;
macro_rules! execute { ($instruction:literal,$a:expr,$b:expr,$c:expr,$control:expr)=>{{let result:u32;let flags:u32;let opcode:u32;unsafe{asm!("msr APSR_nzcvqg, {control}","adr {ptr}, 2f","ldr {opcode}, [{ptr}]","ror {opcode}, {opcode}, #16","2:",$instruction,"mrs {flags}, APSR",in("r1")$a,in("r2")$b,in("r3")$c,lateout("r0")result,control=in(reg)$control,ptr=out(reg)_,opcode=out(reg)opcode,flags=out(reg)flags,options(nostack));}(opcode,result,flags)}};}
fn emit(op: u32, a: u32, b: u32, c: u32, control: u32) {
    let (opcode, result, flags) = match op {
        0 => execute!("smuad r0,r1,r2", a, b, c, control),
        1 => execute!("smuadx r0,r1,r2", a, b, c, control),
        2 => execute!("smusd r0,r1,r2", a, b, c, control),
        3 => execute!("smusdx r0,r1,r2", a, b, c, control),
        4 => execute!("smlad r0,r1,r2,r3", a, b, c, control),
        5 => execute!("smladx r0,r1,r2,r3", a, b, c, control),
        6 => execute!("smlsd r0,r1,r2,r3", a, b, c, control),
        7 => execute!("smlsdx r0,r1,r2,r3", a, b, c, control),
        8 => execute!("smulwb r0,r1,r2", a, b, c, control),
        9 => execute!("smulwt r0,r1,r2", a, b, c, control),
        10 => execute!("smlawb r0,r1,r2,r3", a, b, c, control),
        11 => execute!("smlawt r0,r1,r2,r3", a, b, c, control),
        12 => execute!("smmul r0,r1,r2", a, b, c, control),
        13 => execute!("smmulr r0,r1,r2", a, b, c, control),
        14 => execute!("smmla r0,r1,r2,r3", a, b, c, control),
        15 => execute!("smmlar r0,r1,r2,r3", a, b, c, control),
        16 => execute!("smmls r0,r1,r2,r3", a, b, c, control),
        17 => execute!("smmlsr r0,r1,r2,r3", a, b, c, control),
        18 => execute!("usad8 r0,r1,r2", a, b, c, control),
        19 => execute!("usada8 r0,r1,r2,r3", a, b, c, control),
        20 => execute!("smulbb r0,r1,r2", a, b, c, control),
        21 => execute!("smulbt r0,r1,r2", a, b, c, control),
        22 => execute!("smultb r0,r1,r2", a, b, c, control),
        23 => execute!("smultt r0,r1,r2", a, b, c, control),
        24 => execute!("smlabb r0,r1,r2,r3", a, b, c, control),
        25 => execute!("smlabt r0,r1,r2,r3", a, b, c, control),
        26 => execute!("smlatb r0,r1,r2,r3", a, b, c, control),
        27 => execute!("smlatt r0,r1,r2,r3", a, b, c, control),
        _ => unreachable!(),
    };
    let words = [opcode, a, b, c, control, result, flags, 0x504c554d];
    let hex = b"0123456789abcdef";
    let mut line = [0u8; 66];
    for (i, byte) in words.into_iter().flat_map(u32::to_le_bytes).enumerate() {
        line[2 * i] = hex[(byte >> 4) as usize];
        line[2 * i + 1] = hex[(byte & 15) as usize];
    }
    line[64] = b'\n';
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
    let mut seed = 0x81b6c9d3u32;
    for i in 0..512 {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let a = if i < 256 { values[i / 16] } else { seed };
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let b = if i < 256 { values[i % 16] } else { seed };
        for c in [0u32, 1, 0x7fffffff, 0x80000000] {
            for op in 0..28 {
                for control in [0xa00a0000, 0xa80a0000, 0x50050000, 0x58050000] {
                    emit(op, a, b, c, control);
                }
            }
        }
    }
}
