//! Independent signed long multiply records, including negative operands.
use core::arch::asm;
fn emit(a: u32, b: u32, control: u32) {
    let lo: u32;
    let hi: u32;
    let flags: u32;
    unsafe {
        asm!("msr APSR_nzcvqg, {control}","smull r0,r3,r1,r2","mrs {flags}, APSR",in("r1")a,in("r2")b,out("r0")lo,out("r3")hi,control=in(reg)control,flags=out(reg)flags,options(nostack));
    }
    let words = [0xfb810302, a, b, control, lo, hi, flags, 0x474e4f4c];
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
    let mut seed = 0x91b6c9d3u32;
    for i in 0..512 {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let a = if i < 256 { values[i / 16] } else { seed };
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let b = if i < 256 { values[i % 16] } else { seed };
        for control in [0xa00a0000, 0xa80a0000, 0x50050000, 0x58050000] {
            emit(a, b, control);
        }
    }
}
