//! Literal saturation instructions executed by the independent ARM guest.
use core::arch::asm;
#[repr(align(4))]
struct Code([u16; 4]);
static mut CODE: Code = Code([0; 4]);
fn emit(opcode: u32, input: u32, control: u32) {
    let target = unsafe { core::ptr::addr_of_mut!(CODE.0).cast::<u16>() };
    let result: u32;
    let flags: u32;
    unsafe {
        target.write_volatile((opcode >> 16) as u16);
        target.add(1).write_volatile(opcode as u16);
        target.add(2).write_volatile(0x4770);
        asm!("dsb","isb","mov r1, {input}","msr APSR_nzcvqg, {control}","blx {target}","mov {result}, r0","mrs {flags}, APSR",
            input=in(reg)input,control=in(reg)control,target=in(reg)(target as u32|1),
            result=out(reg)result,flags=out(reg)flags,out("r0")_,out("r1")_,out("lr")_,options(nostack));
    }
    let words = [opcode, input, control, result, flags, 0x54415353];
    let hex = b"0123456789abcdef";
    let mut line = [0u8; 50];
    for (i, byte) in words.into_iter().flat_map(u32::to_le_bytes).enumerate() {
        line[2 * i] = hex[(byte >> 4) as usize];
        line[2 * i + 1] = hex[(byte & 15) as usize];
    }
    line[48] = b'\n';
    unsafe {
        super::call(4, line.as_ptr() as u32);
    }
}
pub(super) fn run() {
    let special = [
        0, 1, 0xffffffff, 0x80000000, 0x7fffffff, 0x00008000, 0x00007fff, 0xffff0000, 0x80007fff,
        0x7fff8000, 0x80008000, 0x7fff7fff, 0x00010001, 0xffff0001, 0x40000000, 0xc0000000,
    ];
    let mut seed = 0xb45f7a21u32;
    for unsigned in [false, true] {
        for lanes in [false, true] {
            for encoded in 0..if lanes { 16 } else { 32 } {
                for shift in 0..if lanes { 1 } else { 63 } {
                    let arithmetic = shift >= 32;
                    let amount = if arithmetic { shift - 31 } else { shift };
                    let base = if lanes { 0xf3210000 } else { 0xf3010000 };
                    let opcode = base
                        | u32::from(unsigned) << 23
                        | u32::from(arithmetic) << 21
                        | (amount >> 2) << 12
                        | (amount & 3) << 6
                        | encoded;
                    for i in 0..32 {
                        seed ^= seed << 13;
                        seed ^= seed >> 17;
                        seed ^= seed << 5;
                        let input = special.get(i).copied().unwrap_or(seed);
                        for flags in [0xa00a0000, 0x50050000] {
                            for sticky in [0, 1 << 27] {
                                emit(opcode, input, flags | sticky);
                            }
                        }
                    }
                }
            }
        }
    }
}
