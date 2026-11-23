//! Fixed-point conversion instructions executed by QEMU, independently of zmu.
use core::arch::asm;

#[repr(align(4))]
struct Code([u16; 4]);
static mut CODE: Code = Code([0; 4]);

fn execute(opcode: u32, bits: u64, control: u32) -> (u32, u32, u32) {
    let target = unsafe { core::ptr::addr_of_mut!(CODE.0).cast::<u16>() };
    let (lo, hi, flags);
    unsafe {
        target.write_volatile((opcode >> 16) as u16);
        target.add(1).write_volatile(opcode as u16);
        target.add(2).write_volatile(0x4770); // BX LR
        asm!("dsb", "isb", "vmov d0, {il}, {ih}", "vmsr fpscr, {control}",
            "blx {target}", "vmov {lo}, {hi}, d0", "vmrs {flags}, fpscr",
            il=in(reg)bits as u32, ih=in(reg)(bits>>32) as u32,
            control=in(reg)control, target=in(reg)(target as u32|1),
            lo=out(reg)lo, hi=out(reg)hi, flags=out(reg)flags,
            out("lr")_, out("d0")_, options(nostack));
    }
    (lo, hi, flags)
}
fn emit(opcode: u32, bits: u64, control: u32) {
    let (lo, hi, flags) = execute(opcode, bits, control);
    let words = [opcode, bits as u32, (bits >> 32) as u32, control, lo, hi, flags, 0x44584946];
    let hex = b"0123456789abcdef";
    let mut line = [0u8; 66];
    for (i, byte) in words.into_iter().flat_map(u32::to_le_bytes).enumerate() {
        line[2*i] = hex[(byte>>4) as usize];
        line[2*i+1] = hex[(byte&15) as usize];
    }
    line[64] = b'\n';
    unsafe { super::call(4, line.as_ptr() as u32); }
}
pub(super) fn run() {
    let singles = [0, 0x80000000, 0x3f800000, 0xbf800000, 0x3fc00000, 0xbfc00000,
        1, 0x80000001, 0x00800000, 0x007fffff, 0x7f800000, 0xff800000,
        0x7fc12345, 0x7f812345, 0xffc12345, 0xff812345, 0x7f7fffff, 0xff7fffff,
        0x3f7fffff, 0x3f800001, 0x3effffff, 0x3f000001, 0x46ffffff, 0x47000000,
        0x477fff00, 0x47800000, 0x4effffff, 0x4f000000, 0x4f7fffff, 0x4f800000,
        0xcf000000, 0xcf000001];
    let doubles = [0, 0x8000000000000000, 0x3ff0000000000000, 0xbff0000000000000,
        0x3ff8000000000000, 0xbff8000000000000, 1, 0x8000000000000001,
        0x0010000000000000, 0x000fffffffffffff, 0x7ff0000000000000, 0xfff0000000000000,
        0x7ff8123456789abc, 0x7ff0123456789abc, 0xfff8123456789abc, 0xfff0123456789abc,
        0x7fefffffffffffff, 0xffefffffffffffff, 0x3fefffffffffffff, 0x3ff0000000000001,
        0x3fdfffffffffffff, 0x3fe0000000000001, 0x40dfffffffffffff, 0x40e0000000000000,
        0x40efffffffffffff, 0x40f0000000000000, 0x41dfffffffffffff, 0x41e0000000000000,
        0x41efffffffffffff, 0x41f0000000000000, 0xc1e0000000000000, 0xc1e0000000000001];
    let integers = [0, 1, 0xffff, 0x8000, 0x7fff, 0x10000, 0x7fffffff, 0x80000000,
        0xffffffff, 0xfffffffe, 0x01000001, 0x01000003, 0x80010001, 0x7fff8000,
        0xdeadbeef, 0xaaaa5555];
    let mut seed = 0x197acb5e31487d06u64;
    for double in [false, true] {
        for to_fixed in [false, true] {
            for unsigned in [false, true] {
                for width in [16u32, 32] {
                    for fbits in if width==16 {0..=16} else {1..=32} {
                        let immediate = width-fbits;
                        let opcode = 0xeeba0a40 | u32::from(double)<<8 | u32::from(to_fixed)<<18
                            | u32::from(unsigned)<<16 | u32::from(width==32)<<7
                            | (immediate>>1) | (immediate&1)<<5;
                        for i in 0..64 {
                            seed ^= seed<<13; seed ^= seed>>7; seed ^= seed<<17;
                            let bits = if to_fixed {
                                if double { doubles.get(i).copied().unwrap_or(seed) }
                                else { u64::from(singles.get(i).copied().unwrap_or(seed as u32)) | 0xa5a55a5a00000000 }
                            } else {
                                u64::from(integers.get(i).copied().unwrap_or(seed as u32)) | 0xdeadbeef00000000
                            };
                            for rounding in 0..4 {
                                for mode in 0..4 {
                                    let control = 0xa0000000 | rounding<<22 | (mode&1)<<24 | (mode>>1)<<25;
                                    emit(opcode,bits,control);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
