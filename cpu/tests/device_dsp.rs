#![cfg(feature = "has-dsp-ext")]
use zmu_cortex_m::{
    Processor,
    core::{
        register::{BaseReg, Reg},
        reset::Reset,
    },
    executor::Executor,
};

#[test]
fn keyboard_packed_saturation_executes_both_lanes_and_preserves_all_flags() {
    // ARM DUI/Armv7 instruction guide: UQADD16 and UQSUB16 leave NZCVQ and GE unchanged.
    // These literal expectations cover saturation without carry/borrow crossing lanes.
    for (add, a, b, expected) in [
        (true, 0xffff0001, 0x00020003, 0xffff0004),
        (false, 0xffff0001, 0x00020003, 0xfffd0000),
        (true, 0x0000ffff, 0xffff0001, 0xffffffff),
        (false, 0x0000ffff, 0xffff0001, 0x0000fffe),
        (true, 0x00010002, 0x00030004, 0x00040006),
        (false, 0x00010002, 0x00030004, 0),
    ] {
        let opcode: u32 = if add { 0xfa90f253 } else { 0xfad0f253 };
        let mut image = vec![0; 32];
        image[..4].copy_from_slice(&0x20010000_u32.to_le_bytes());
        image[4..8].copy_from_slice(&9_u32.to_le_bytes());
        image[8..10].copy_from_slice(&((opcode >> 16) as u16).to_le_bytes());
        image[10..12].copy_from_slice(&(opcode as u16).to_le_bytes());
        let mut cpu = Processor::new();
        cpu.flash_memory(image.len(), &image);
        cpu.reset().unwrap();
        cpu.set_r(Reg::R0, a);
        cpu.set_r(Reg::R3, b);
        cpu.psr.value |= 0xa80f0000;
        let flags = cpu.psr.value;
        cpu.step();
        assert_eq!(cpu.get_r(Reg::R2), expected);
        assert_eq!(cpu.psr.value, flags);
        assert_eq!(cpu.get_pc(), 12);
    }
}
