#![cfg(feature = "has-fp")]
use zmu_cortex_m::{
    Processor,
    core::{instruction::Instruction, thumb::ThumbCode},
    decoder::Decoder,
    executor::Executor,
};

fn check(precision: u32, operation: u32) {
    let data = include_bytes!("../fixtures/isa_oracle_accumulate.bin");
    assert_eq!(data.len(), 49_152 * 52);
    if precision == 64 && !cfg!(feature = "fpv5-d16") {
        return;
    }
    let opcode = if precision == 32 {
        [0xee000a81, 0xee000ac1, 0xee100a81, 0xee100ac1]
    } else {
        [0xee010b02, 0xee010b42, 0xee110b02, 0xee110b42]
    }[operation as usize];
    for native in [false, true] {
        for (compiled, sticky) in [(false, 0), (true, 0), (false, 0x9f), (true, 0x9f)] {
            let mut cpu = Processor::new();
            cpu.cpacr = 0x00f0_0000;
            cpu.native_float = native;
            let instruction = cpu.decode(ThumbCode::Thumb32 { opcode });
            assert!(
                !matches!(instruction, Instruction::UDF { .. }),
                "missing {opcode:08x}"
            );
            for record in data.as_chunks::<52>().0 {
                let words: [u32; 13] = core::array::from_fn(|i| {
                    u32::from_le_bytes(record[i * 4..i * 4 + 4].try_into().unwrap())
                });
                let [
                    width,
                    al,
                    ah,
                    bl,
                    bh,
                    cl,
                    ch,
                    op,
                    control,
                    lo,
                    hi,
                    status,
                    magic,
                ] = words;
                assert_eq!(magic, 0x414c_4d56);
                if width != precision || op != operation {
                    continue;
                }
                cpu.fpscr = control | sticky;
                cpu.psr.value = 0xf90a_0000;
                cpu.fp_regs.fill(0xdead_beef);
                if precision == 32 {
                    cpu.fp_regs[..3].copy_from_slice(&[cl, al, bl]);
                } else {
                    cpu.fp_regs[..6].copy_from_slice(&[cl, ch, al, ah, bl, bh]);
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
                    (lo, hi, status | sticky),
                    "{instruction:?} a={ah:08x}:{al:08x} b={bh:08x}:{bl:08x} c={ch:08x}:{cl:08x} control={control:08x} native={native} compiled={compiled}"
                );
                assert_eq!((cpu.cfsr, cpu.hfsr), (0, 0));
                assert_eq!(cpu.psr.value & 0xf80f_0000, 0xf80a_0000);
                let start = if precision == 32 { 1 } else { 2 };
                assert_eq!(cpu.fp_regs[start..], before[start..]);
            }
        }
    }
}
macro_rules! cases {($(($name:ident,$p:expr,$op:expr)),*$(,)?)=>{$(#[test]fn $name(){check($p,$op);})*};}
cases! {(single_mla,32,0),(single_mls,32,1),(single_nmls,32,2),(single_nmla,32,3),
(double_mla,64,0),(double_mls,64,1),(double_nmls,64,2),(double_nmla,64,3)}
