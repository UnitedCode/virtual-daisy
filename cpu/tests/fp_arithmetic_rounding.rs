#![cfg(feature = "has-fp")]
use zmu_cortex_m::{
    Processor,
    core::{instruction::Instruction, thumb::ThumbCode},
    decoder::Decoder,
    executor::Executor,
};

fn check(precision: u32, operation: u32) {
    assert_eq!(
        include_bytes!("../fixtures/isa_oracle_arithmetic_rounding.bin").len(),
        61_440 * 44
    );
    if precision == 64 && !cfg!(feature = "fpv5-d16") {
        return;
    }
    let single = [0xee300a81, 0xee300ac1, 0xee200a81, 0xee800a81, 0xeeb10ae0];
    let double = [0xee310b02, 0xee310b42, 0xee210b02, 0xee810b02, 0xeeb10bc1];
    for native in [false, true] {
        for compiled in [false, true] {
            let mut cpu = Processor::new();
            cpu.cpacr = 0x00f0_0000;
            cpu.native_float = native;
            let opcode = if precision == 32 { single } else { double }[operation as usize];
            let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
            assert!(
                !matches!(instruction, Instruction::UDF { .. }),
                "{opcode:08x}"
            );
            for record in include_bytes!("../fixtures/isa_oracle_arithmetic_rounding.bin")
                .as_chunks::<44>()
                .0
            {
                let words: [u32; 11] = core::array::from_fn(|i| {
                    u32::from_le_bytes(record[i * 4..i * 4 + 4].try_into().unwrap())
                });
                let [width, al, ah, bl, bh, op, control, lo, hi, status, magic] = words;
                assert_eq!(magic, 0x4854_5241);
                if width != precision || op != operation {
                    continue;
                }
                cpu.psr.value = 0xf90a_0000;
                cpu.fpscr = control;
                cpu.fp_regs.fill(0xdead_beef);
                if precision == 32 {
                    cpu.fp_regs[1] = al;
                    cpu.fp_regs[2] = bl;
                } else {
                    cpu.fp_regs[2..6].copy_from_slice(&[al, ah, bl, bh]);
                }
                let before = cpu.fp_regs;
                if compiled {
                    cpu.execute_compiled(&instruction, 4);
                } else {
                    cpu.execute(&instruction, 4);
                }
                let result_hi = if precision == 32 { 0 } else { cpu.fp_regs[1] };
                assert_eq!(
                    (cpu.fp_regs[0], result_hi, cpu.fpscr),
                    (lo, hi, status),
                    "{instruction:?} a={ah:08x}:{al:08x} b={bh:08x}:{bl:08x} control={control:08x} native={native} compiled={compiled}"
                );
                assert_eq!((cpu.cfsr, cpu.hfsr), (0, 0));
                assert_eq!(cpu.psr.value & 0xf80f_0000, 0xf80a_0000);
                let start = if precision == 32 { 1 } else { 2 };
                assert_eq!(
                    cpu.fp_regs[start..],
                    before[start..],
                    "non-destination registers"
                );
            }
        }
    }
}

macro_rules! tests {
    ($(($name:ident, $precision:expr, $operation:expr)),* $(,)?) => {
        $(#[test] fn $name() { check($precision, $operation); })*
    };
}
tests! {
    (single_add_all_rounding_modes, 32, 0),
    (single_subtract_all_rounding_modes, 32, 1),
    (single_multiply_all_rounding_modes, 32, 2),
    (single_divide_all_rounding_modes, 32, 3),
    (single_square_root_all_rounding_modes, 32, 4),
    (double_add_all_rounding_modes, 64, 0),
    (double_subtract_all_rounding_modes, 64, 1),
    (double_multiply_all_rounding_modes, 64, 2),
    (double_divide_all_rounding_modes, 64, 3),
    (double_square_root_all_rounding_modes, 64, 4),
}
