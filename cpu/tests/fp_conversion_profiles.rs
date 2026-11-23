use zmu_cortex_m::{
    Processor,
    core::{instruction::Instruction, thumb::ThumbCode},
    decoder::Decoder,
};
#[test]
fn fixed_conversions_require_the_implemented_precision() {
    let cpu = Processor::new();
    for base in [0xeeba0a40, 0xeebb0a48, 0xeebe0ac0, 0xeebf0acf] {
        for double in [false, true] {
            let opcode = base | u32::from(double) << 8;
            assert_eq!(
                !matches!(
                    cpu.decode(ThumbCode::Thumb32 { opcode }),
                    Instruction::UDF { .. }
                ),
                if double {
                    cfg!(feature = "fpv5-d16")
                } else {
                    cfg!(feature = "has-fp")
                },
                "{opcode:08x}"
            );
        }
    }
}
