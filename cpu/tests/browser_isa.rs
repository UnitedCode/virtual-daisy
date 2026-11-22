//! Regression coverage for the browser backend's CPU changes.
#![cfg(feature = "has-fp")]
use zmu_cortex_m::{
    Processor,
    bus::Bus,
    core::{
        instruction::*,
        register::{Reg, SingleReg},
        reset::Reset,
    },
    executor::Executor,
    peripheral::nvic::NVIC,
};
fn cpu() -> Processor {
    let mut c = Processor::new();
    c.cpacr = 0x00f00000;
    c
}
#[test]
fn float_results_and_flags_match_qemu_arm_isa() {
    let p = VAddSubParamsf32 {
        sd: SingleReg::S0,
        sn: SingleReg::S1,
        sm: SingleReg::S2,
    };
    let mut fast = cpu();
    for record in include_bytes!("../fixtures/isa_oracle.bin").chunks_exact(20) {
        let words: Vec<u32> = record
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        let [a, b, op, value, flags] = words[..] else {
            unreachable!()
        };
        let instruction = [
            Instruction::VADD_f32 { params: p },
            Instruction::VSUB_f32 { params: p },
            Instruction::VMUL_f32 { params: p },
            Instruction::VDIV_f32 { params: p },
            Instruction::VSQRT_f32 {
                params: VMovRegParamsf32 {
                    sd: SingleReg::S0,
                    sm: SingleReg::S1,
                },
            },
        ][op as usize];
        fast.fpscr = 0;
        fast.fp_regs[1] = a;
        fast.fp_regs[2] = b;
        fast.execute(&instruction, 4);
        assert_eq!(
            (fast.fp_regs[0], fast.fpscr),
            (value, flags),
            "{instruction:?}: {a:08x} {b:08x}"
        );
        fast.fpscr = 0;
        fast.fp_regs[1] = a;
        fast.fp_regs[2] = b;
        fast.execute_compiled(&instruction, 4);
        assert_eq!(
            (fast.fp_regs[0], fast.fpscr),
            (value, flags),
            "decoded {instruction:?}: {a:08x} {b:08x}"
        );
        fast.fpscr = 16;
        fast.fp_regs[1] = a;
        fast.fp_regs[2] = b;
        fast.execute(&instruction, 4);
        assert_eq!(
            (fast.fp_regs[0], fast.fpscr),
            (value, flags | 16),
            "sticky IXC {instruction:?}: {a:08x} {b:08x}"
        );
    }
}
#[test]
fn binary32_special_results_and_controls_match_qemu_arm_isa() {
    let params = VAddSubParamsf32 {
        sd: SingleReg::S0,
        sn: SingleReg::S1,
        sm: SingleReg::S2,
    };
    let instructions = [
        Instruction::VADD_f32 { params },
        Instruction::VSUB_f32 { params },
        Instruction::VMUL_f32 { params },
        Instruction::VDIV_f32 { params },
        Instruction::VSQRT_f32 {
            params: VMovRegParamsf32 {
                sd: SingleReg::S0,
                sm: SingleReg::S1,
            },
        },
        Instruction::VCMP_f32 {
            params: VCmpParamsf32 {
                sd: SingleReg::S1,
                sm: SingleReg::S2,
                quiet_nan_exc: false,
                with_zero: false,
            },
        },
        Instruction::VCMP_f32 {
            params: VCmpParamsf32 {
                sd: SingleReg::S1,
                sm: SingleReg::S2,
                quiet_nan_exc: true,
                with_zero: false,
            },
        },
    ];
    let mut fast = cpu();
    for record in include_bytes!("../fixtures/isa_oracle_control.bin").chunks_exact(24) {
        let words = record
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect::<Vec<_>>();
        let [a, b, op, control, value, flags] = words[..] else {
            unreachable!()
        };
        for sticky in [0, 16] {
            for decoded in [false, true] {
                fast.fpscr = control | sticky;
                fast.fp_regs[1] = a;
                fast.fp_regs[2] = b;
                if decoded {
                    fast.execute_compiled(&instructions[op as usize], 4);
                } else {
                    fast.execute(&instructions[op as usize], 4);
                }
                assert_eq!(
                    (if op < 5 { fast.fp_regs[0] } else { value }, fast.fpscr),
                    (value, flags | sticky),
                    "op={op} {a:08x} {b:08x} control={control:08x} decoded={decoded}"
                );
            }
        }
    }
}
#[test]
fn binary64_results_and_flags_match_qemu_arm_isa() {
    use zmu_cortex_m::core::register::DoubleReg::*;
    let params = VAddSubParamsf64 {
        dd: D0,
        dn: D1,
        dm: D2,
    };
    let instructions = [
        Instruction::VADD_f64 { params },
        Instruction::VSUB_f64 { params },
        Instruction::VMUL_f64 { params },
        Instruction::VDIV_f64 { params },
        Instruction::VCMP_f64 {
            params: VCmpParamsf64 {
                dd: D1,
                dm: D2,
                quiet_nan_exc: false,
                with_zero: false,
            },
        },
    ];
    let mut fast = cpu();
    for record in include_bytes!("../fixtures/isa_oracle_f64.bin").chunks_exact(32) {
        let words = record
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect::<Vec<_>>();
        let [al, ah, bl, bh, op, lo, hi, flags] = words[..] else {
            unreachable!()
        };
        let instruction = instructions[op as usize];
        for sticky in [0, 16] {
            for decoded in [false, true] {
                fast.fpscr = sticky;
                fast.fp_regs[2] = al;
                fast.fp_regs[3] = ah;
                fast.fp_regs[4] = bl;
                fast.fp_regs[5] = bh;
                if decoded {
                    fast.execute_compiled(&instruction, 4);
                } else {
                    fast.execute(&instruction, 4);
                }
                if op < 4 {
                    assert_eq!(
                        (fast.fp_regs[0], fast.fp_regs[1]),
                        (lo, hi),
                        "{instruction:?}: {ah:08x}{al:08x} {bh:08x}{bl:08x}"
                    );
                }
                assert_eq!(
                    fast.fpscr,
                    flags | sticky,
                    "FPSCR {instruction:?}: {ah:08x}{al:08x} {bh:08x}{bl:08x}"
                );
            }
        }
    }
}
#[test]
fn binary64_multiply_accumulate_keeps_two_roundings() {
    use zmu_cortex_m::core::register::DoubleReg::*;
    let mut c = cpu();
    c.fp_regs[0] = 0;
    c.fp_regs[1] = 0xbff00000;
    c.fp_regs[2] = 1;
    c.fp_regs[3] = 0x3ff00000;
    c.fp_regs[4] = 0xffff_fffe;
    c.fp_regs[5] = 0x3fefffff;
    c.execute(
        &Instruction::VMLA_f64 {
            params: VAddSubParamsf64 {
                dd: D0,
                dn: D1,
                dm: D2,
            },
            subtract: false,
            negate: false,
        },
        4,
    );
    assert_eq!((c.fp_regs[0], c.fp_regs[1], c.fpscr), (0, 0, 16));
}
#[test]
fn lazy_decode_does_not_decode_embedded_data() {
    let mut image = vec![0; 32];
    image[..4].copy_from_slice(&0x20010000u32.to_le_bytes());
    image[4..8].copy_from_slice(&9u32.to_le_bytes());
    image[8..12].copy_from_slice(&[0x01, 0x20, 0xfe, 0xe7]);
    // A valid VMOV transfer that upstream's eager decoder cannot decode.
    image[12..16].copy_from_slice(&[0x50, 0xec, 0x10, 0x0a]);
    let mut c = cpu();
    c.flash_memory(image.len(), &image);
    c.cache_instructions();
    c.reset().unwrap();
    c.step();
    assert_eq!(c.r0_12[0], 1);
}
#[test]
fn nvic_register_banks_and_byte_priorities_remain_independent() {
    let mut c = cpu();
    for n in 0..160 {
        let e = zmu_cortex_m::core::exception::Exception::Interrupt { n };
        c.exceptions
            .entry(usize::from(e))
            .or_insert_with(|| zmu_cortex_m::core::exception::ExceptionState::new(e, 0));
    }
    c.write32(0xe000e104, 1 << 5).unwrap();
    assert_eq!(c.nvic_read_iser(1), 1 << 5);
    assert_eq!(c.nvic_read_iser(0), 0);
    c.write8(0xe000e400 + 37, 0x30).unwrap();
    assert_eq!(c.read8(0xe000e400 + 37).unwrap(), 0x30);
    assert_eq!(c.read8(0xe000e400 + 36).unwrap(), 0);
    c.write32(0xe000e204, 1 << 5).unwrap();
    assert_eq!(c.nvic_read_ispr(1), 1 << 5);
}
#[test]
fn multiply_accumulate_rounds_product_before_add() {
    let mut c = cpu();
    let p = VAddSubParamsf32 {
        sd: SingleReg::S0,
        sn: SingleReg::S1,
        sm: SingleReg::S2,
    };
    c.fp_regs[0] = (-1.0f32).to_bits();
    c.fp_regs[1] = f32::from_bits(0x3f800001).to_bits();
    c.fp_regs[2] = f32::from_bits(0x3f7ffffe).to_bits();
    c.execute(
        &Instruction::VMLA_f32 {
            params: p,
            subtract: false,
            negate: false,
        },
        4,
    );
    assert_eq!(c.fp_regs[0], 0); // A fused FMA would retain the tiny product error.
}
#[test]
fn uxtb16_rotates_and_extracts_both_bytes() {
    let mut c = cpu();
    c.r0_12[1] = 0x12345678;
    c.execute(
        &Instruction::UXTB16 {
            params: Reg2UsizeParams {
                rd: Reg::R0,
                rm: Reg::R1,
                rotation: 8,
            },
        },
        4,
    );
    assert_eq!(c.r0_12[0], 0x00120056);
}
#[test]
fn mvn_without_s_preserves_flags() {
    use zmu_cortex_m::{core::thumb::ThumbCode, decoder::Decoder};
    let mut c = cpu();
    c.psr.value = 0xa0000000;
    let i = c.decode(ThumbCode::Thumb32 { opcode: 0xf06f4141 });
    c.execute(&i, 4);
    assert_eq!(c.r0_12[1], !0xc1000000);
    assert_eq!(c.psr.value, 0xa0000000);
}
#[test]
fn conditional_float_moves_obey_it_and_do_not_activate_fp_when_skipped() {
    use zmu_cortex_m::{core::thumb::ThumbCode, decoder::Decoder};
    let mut c = cpu();
    c.psr.value = 1 << 30;
    c.fp_regs[0] = 0x12345678;
    c.fp_regs[1] = 0x87654321;
    let it = c.decode(ThumbCode::Thumb16 { opcode: 0xbf18 });
    c.execute(&it, 2);
    c.cpacr = 0;
    c.execute(
        &Instruction::VMOV_reg_f32 {
            params: VMovRegParamsf32 {
                sd: SingleReg::S0,
                sm: SingleReg::S1,
            },
        },
        4,
    );
    assert_eq!(c.fp_regs[0], 0x12345678);
    assert_eq!(c.get_itstate(), 0);
    assert_eq!(c.cfsr, 0);
}
#[test]
fn coalesced_decrement_loops_match_instruction_execution() {
    use zmu_cortex_m::core::register::BaseReg;
    for value in [2, 1000, 0x80000000, 0x80000001, 0xffffffff] {
        let make = || {
            let mut c = cpu();
            let mut image = vec![0; 32];
            image[..4].copy_from_slice(&0x20010000u32.to_le_bytes());
            image[4..8].copy_from_slice(&9u32.to_le_bytes());
            image[8..12].copy_from_slice(&[1, 0x38, 0xfd, 0xd1]);
            c.flash_memory(image.len(), &image);
            c.reset().unwrap();
            c.r0_12[0] = value;
            c
        };
        let mut a = make();
        let mut b = make();
        assert!(a.advance_decrement_loop(40));
        for _ in 0..a.instruction_count {
            b.step();
        }
        assert_eq!(
            (a.r0_12, a.psr.value, a.get_pc(), a.cycle_count),
            (b.r0_12, b.psr.value, b.get_pc(), b.cycle_count)
        );
    }
}
#[test]
fn coalesced_nop_calls_preserve_registers_flags_and_counts() {
    use zmu_cortex_m::core::register::BaseReg;
    let make = || {
        let mut c = cpu();
        let mut image = vec![0; 32];
        image[..4].copy_from_slice(&0x20010000u32.to_le_bytes());
        image[4..8].copy_from_slice(&9u32.to_le_bytes());
        image[8..14].copy_from_slice(&[0, 0xf0, 4, 0xf8, 0xfc, 0xe7]);
        image[20..24].copy_from_slice(&[0, 0xbf, 0x70, 0x47]);
        c.flash_memory(image.len(), &image);
        c.reset().unwrap();
        c
    };
    let mut a = make();
    let mut b = make();
    assert!(a.advance_nop_call_loop(110));
    for _ in 0..40 {
        b.step();
    }
    assert_eq!(
        (
            a.r0_12,
            a.psr.value,
            a.lr,
            a.get_pc(),
            a.cycle_count,
            a.instruction_count
        ),
        (
            b.r0_12,
            b.psr.value,
            b.lr,
            b.get_pc(),
            b.cycle_count,
            b.instruction_count
        )
    );
}
