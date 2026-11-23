//! Actual unprivileged ARM transfers executed by QEMU, including every imm8.
use core::{
    arch::asm,
    ptr::{read_volatile, write_volatile},
};
const BASE: u32 = 0x20001004;
const CODE: u32 = 0x20030000;
macro_rules! template {($instruction:literal)=>{{let opcode:u32;unsafe{asm!("adr {ptr},2f","ldr {opcode},[{ptr}]","ror {opcode},{opcode},#16","b 3f","2:",$instruction,"3:",ptr=out(reg)_,opcode=out(reg)opcode,options(nostack));}opcode}};}
fn opcode(operation: usize) -> u32 {
    match operation {
        0 => template!("ldrt r0,[r1,#0]"),
        1 => template!("ldrbt r0,[r1,#0]"),
        2 => template!("ldrht r0,[r1,#0]"),
        3 => template!("ldrsbt r0,[r1,#0]"),
        4 => template!("ldrsht r0,[r1,#0]"),
        5 => template!("strt r0,[r1,#0]"),
        6 => template!("strbt r0,[r1,#0]"),
        7 => template!("strht r0,[r1,#0]"),
        _ => unreachable!(),
    }
}
pub(super) fn run() {
    let seeds = [
        0u32, 1, 0xffffffff, 0x7fffffff, 0x80000000, 0x7fff7fff, 0x80008000, 0x7fff8000,
        0x80007fff, 0x0000ffff, 0xffff0000, 0x00ff00ff, 0xff00ff00, 0x01234567, 0xc0000000,
        0x5555aaaa,
    ];
    for seed in seeds {
        for offset in 0..256u32 {
            for operation in 0..8 {
                let instruction = opcode(operation) | offset;
                for source in [seed, BASE] {
                    for control in [0xa00a0000u32, 0xa80a0000, 0x50050000, 0x58050000] {
                        for i in 0..8u32 {
                            let index = offset + 3 + i;
                            let byte = (seed.rotate_right((index % 4) * 8) as u8)
                                ^ (index.wrapping_mul(37) as u8);
                            unsafe {
                                write_volatile((BASE + offset - 1 + i) as *mut u8, byte);
                            }
                        }
                        let mut result = source;
                        let status: u32;
                        unsafe {
                            write_volatile(CODE as *mut u16, (instruction >> 16) as u16);
                            write_volatile((CODE + 2) as *mut u16, instruction as u16);
                            write_volatile((CODE + 4) as *mut u16, 0x4770);
                            asm!("dsb", "isb", options(nostack));
                            asm!("msr APSR_nzcvqg,{control}","blx r3","mrs {status},APSR",inout("r0")result,in("r1")BASE,in("r3")CODE|1,control=in(reg)control,status=out(reg)status,out("lr")_,options(nostack));
                        }
                        let bytes: [u8; 8] = core::array::from_fn(|i| unsafe {
                            read_volatile((BASE + offset - 1 + i as u32) as *const u8)
                        });
                        let lo = u32::from_le_bytes(bytes[..4].try_into().unwrap());
                        let hi = u32::from_le_bytes(bytes[4..].try_into().unwrap());
                        let words = [
                            instruction,
                            seed,
                            source,
                            control,
                            result,
                            lo,
                            hi,
                            status,
                            0x56525055,
                        ];
                        let hex = b"0123456789abcdef";
                        let mut line = [0u8; 74];
                        for (i, b) in words.into_iter().flat_map(u32::to_le_bytes).enumerate() {
                            line[i * 2] = hex[(b >> 4) as usize];
                            line[i * 2 + 1] = hex[(b & 15) as usize];
                        }
                        line[72] = b'\n';
                        unsafe {
                            super::call(4, line.as_ptr() as u32);
                        }
                    }
                }
            }
        }
    }
}
