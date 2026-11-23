use zmu_cortex_m::{
    Processor,
    bus::Bus,
    core::{register::BaseReg, thumb::ThumbCode},
    decoder::Decoder,
    executor::Executor,
};
#[test]
fn literal_load_to_pc_branches_without_advancing_the_destination() {
    for compiled in [false, true] {
        let mut cpu = Processor::new();
        cpu.branch_write_pc(0x20000000);
        cpu.write32(0x20000004, 0x20001001).unwrap();
        let instruction = cpu.decode(ThumbCode::Thumb32 { opcode: 0xf8dff000 });
        if compiled {
            cpu.execute_compiled(&instruction, 4);
        } else {
            cpu.execute(&instruction, 4);
        }
        assert_eq!(cpu.get_pc(), 0x20001000);
        assert_eq!((cpu.cfsr, cpu.hfsr), (0, 0));
    }
}
