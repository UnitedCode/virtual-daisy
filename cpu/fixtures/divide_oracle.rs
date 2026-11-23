//! Independently executed integer divisions, including MIN/-1 and zero divisors.
use core::{
    arch::asm,
    ptr::{read_volatile, write_volatile},
};
macro_rules! execute {($instruction:literal,$a:expr,$b:expr,$control:expr)=>{{let opcode:u32;let result:u32;let status:u32;unsafe{asm!("msr APSR_nzcvqg,{control}","adr {ptr},2f","ldr {opcode},[{ptr}]","ror {opcode},{opcode},#16","2:",$instruction,"mrs {status},APSR",in("r1")$a,in("r2")$b,lateout("r0")result,control=in(reg)$control,ptr=out(reg)_,opcode=out(reg)opcode,status=out(reg)status,options(nostack));}(opcode,result,status)}};}
pub(super) fn run() {
    unsafe {
        let ccr = read_volatile(0xe000ed14 as *const u32);
        write_volatile(0xe000ed14 as *mut u32, ccr & !16);
        asm!("dsb", "isb", options(nostack));
    }
    let values = [
        0u32, 1, 0xffffffff, 0x7fffffff, 0x80000000, 3, 0xfffffff9, 2, 0x80008000, 0x7fff8000,
        0x0000ffff, 0xffff0000, 0x12345678, 0x00ff00ff, 0xff00ff00, 0x5555aaaa,
    ];
    let mut seed = 0xd1b1a7e5u32;
    for i in 0..512 {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let a = if i < 256 { values[i / 16] } else { seed };
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let b = if i < 256 { values[i % 16] } else { seed };
        for control in [0xa00a0000u32, 0xa80a0000, 0x50050000, 0x58050000] {
            for (opcode, result, status) in [
                execute!("sdiv r0,r1,r2", a, b, control),
                execute!("udiv r0,r1,r2", a, b, control),
            ] {
                let words = [opcode, a, b, control, result, status, 0x49564944];
                let hex = b"0123456789abcdef";
                let mut line = [0u8; 58];
                for (i, b) in words.into_iter().flat_map(u32::to_le_bytes).enumerate() {
                    line[i * 2] = hex[(b >> 4) as usize];
                    line[i * 2 + 1] = hex[(b & 15) as usize];
                }
                line[56] = b'\n';
                unsafe {
                    super::call(4, line.as_ptr() as u32);
                }
            }
        }
    }
}
