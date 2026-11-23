#![cfg(feature = "has-fp")]
use zmu_cortex_m::{
    Processor,
    core::{instruction::Instruction, thumb::ThumbCode},
    decoder::Decoder,
    executor::Executor,
};

fn execute(c: &mut Processor, opcode: u32) {
    let instruction = c.decode(ThumbCode::Thumb32 { opcode });
    c.execute(&instruction, 4);
}
#[test]
fn paired_single_moves_cover_odd_and_high_register_pairs_without_rounding() {
    for first in [0u32, 1, 15, 16, 29, 30] {
        let mut c = Processor::new();
        c.cpacr = 0x00f00000;
        c.r0_12[0] = 0x7f800001;
        c.r0_12[1] = 0x80000000;
        c.fpscr = 0x80000010;
        let register = (first >> 1) | ((first & 1) << 5);
        execute(&mut c, 0xec410a10 | register); // r0,r1 -> Sm,Sm+1
        execute(&mut c, 0xec532a10 | register); // Sm,Sm+1 -> r2,r3
        assert_eq!(&c.r0_12[2..4], &[0x7f800001, 0x80000000]);
        assert_eq!(
            c.fpscr, 0x80000010,
            "bit moves preserve NaN payload and flags"
        );
    }
}
#[test]
fn scalar_words_preserve_the_other_half_of_the_double_register() {
    for d in [0u32, 7, 15] {
        for high in [0u32, 1] {
            let mut c = Processor::new();
            c.cpacr = 0x00f00000;
            c.fp_regs.fill(0x11223344);
            c.r0_12[0] = 0xffc01234;
            let register = (d << 16) | (high << 21);
            execute(&mut c, 0xee000b10 | register);
            execute(&mut c, 0xee101b10 | register);
            assert_eq!(c.r0_12[1], 0xffc01234);
            assert_eq!(c.fp_regs[(d * 2 + (high ^ 1)) as usize], 0x11223344);
            assert_eq!(c.fpscr, 0);
        }
    }
}
#[test]
fn unpredictable_register_encodings_are_undefined_without_panicking() {
    let c = Processor::new();
    for opcode in [
        0xec410a3f, 0xec500a10, 0xec41fa10, 0xec4d0a10, 0xee00db10, 0xee00fb10, 0xee000b90,
    ] {
        assert!(
            matches!(
                c.decode(ThumbCode::Thumb32 { opcode }),
                Instruction::UDF { .. }
            ),
            "{opcode:08x}"
        );
    }
}
