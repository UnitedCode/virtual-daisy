use std::{cell::RefCell, rc::Rc};
use zmu_cortex_m::{
    Processor,
    bus::Bus,
    core::{
        fault::Fault,
        register::{BaseReg, Reg},
        reset::Reset,
    },
    executor::Executor,
};

#[derive(Default)]
struct State {
    waiting: bool,
    ready: bool,
    writes: Vec<(u32, u32)>,
    barriers: Vec<u32>,
    configurations: Vec<u32>,
}
struct Device(Rc<RefCell<State>>);
impl Bus for Device {
    fn systick_reference_divider(&self) -> u32 {
        8
    }
    fn core_config_changed(&mut self, address: u32, _value: u32) -> Result<(), Fault> {
        assert_eq!(
            address, 0xe000ed14,
            "legacy cache callback was extended without opt-in"
        );
        self.0.borrow_mut().configurations.push(address);
        Ok(())
    }
    fn transactional(&self) -> bool {
        true
    }
    fn begin_instruction(&self, _: u32, _: u32, _: bool) {
        self.0.borrow_mut().waiting = false;
    }
    fn instruction_stalled(&self) -> bool {
        self.0.borrow().waiting
    }
    fn memory_barrier(&self, kind: u32) -> Result<(), Fault> {
        self.0.borrow_mut().barriers.push(kind);
        Ok(())
    }
    fn read32(&mut self, _: u32) -> Result<u32, Fault> {
        Err(Fault::Preciserr)
    }
    fn read16(&self, _: u32) -> Result<u16, Fault> {
        Ok(0x2002)
    } // data view: movs r0,#2
    fn read8(&self, _: u32) -> Result<u8, Fault> {
        Err(Fault::Preciserr)
    }
    fn fetch16_at(&self, _: u32, _: u64) -> Result<(u16, bool), Fault> {
        Ok((0x2001, false))
    }
    fn write32(&mut self, a: u32, v: u32) -> Result<(), Fault> {
        let mut s = self.0.borrow_mut();
        if a == 0x40000004 && !s.ready {
            s.waiting = true;
            return Err(Fault::Preciserr);
        }
        if !s.writes.contains(&(a, v)) {
            s.writes.push((a, v));
        }
        Ok(())
    }
    fn write16(&mut self, _: u32, _: u16) -> Result<(), Fault> {
        Err(Fault::Preciserr)
    }
    fn write8(&mut self, _: u32, _: u8) -> Result<(), Fault> {
        Err(Fault::Preciserr)
    }
    fn in_range(&self, a: u32) -> bool {
        (0x40000000..0x40000008).contains(&a)
    }
}
fn fixture() -> (Processor, Rc<RefCell<State>>) {
    let mut bytes = vec![0; 0x100];
    bytes[..4].copy_from_slice(&0x20010000u32.to_le_bytes());
    bytes[4..8].copy_from_slice(&0x81u32.to_le_bytes());
    bytes[64..68].copy_from_slice(&[99, 0x23, 0xfe, 0xe7]); // IRQ: movs r3,#99; b .
    bytes[16 * 4..17 * 4].copy_from_slice(&0x41u32.to_le_bytes());
    // Put handler after its vector entry.
    bytes[16 * 4..17 * 4].copy_from_slice(&0x61u32.to_le_bytes());
    bytes[0x60..0x64].copy_from_slice(&[99, 0x23, 0xfe, 0xe7]);
    bytes[0x80..0x84].copy_from_slice(&[6, 0xc0, 0x00, 0xbf]); // stmia r0!,{r1,r2}; nop
    let s = Rc::new(RefCell::new(State::default()));
    let mut c = Processor::new();
    c.flash_memory(bytes.len(), &bytes);
    c.device(Some(Box::new(Device(s.clone()))));
    c.reset().unwrap();
    c.set_r(Reg::R0, 0x40000000);
    c.set_r(Reg::R1, 11);
    c.set_r(Reg::R2, 22);
    (c, s)
}

#[test]
fn existing_cache_configuration_adapters_do_not_receive_new_register_notifications() {
    let (mut cpu, state) = fixture();
    cpu.write32(0xe000edfc, 1 << 24).unwrap();
    cpu.write32(0xe000ed14, 1 << 9).unwrap();
    assert_eq!(state.borrow().configurations, vec![0xe000ed14]);
}

#[test]
fn switching_systick_source_does_not_turn_old_fractional_cycles_into_ticks() {
    use zmu_cortex_m::peripheral::systick::SysTick;
    let (mut cpu, _) = fixture();
    cpu.syst_write_rvr(99);
    cpu.syst_write_cvr(0);
    cpu.syst_write_csr(1);
    cpu.syst_step(7);
    assert_eq!(cpu.syst_read_cvr(), 0);
    cpu.syst_write_csr(5);
    cpu.syst_step(1);
    assert_eq!(cpu.syst_read_cvr(), 99);
}
#[test]
fn a_deferred_stm_retries_without_retiring_or_repeating_a_completed_write() {
    let (mut c, s) = fixture();
    c.step();
    assert_eq!(c.instruction_count, 0);
    assert_eq!(c.get_pc(), 0x80);
    assert_eq!(c.get_r(Reg::R0), 0x40000000);
    assert!(c.take_pending_fault_trap().is_none());
    assert_eq!(s.borrow().writes, [(0x40000000, 11)]);
    s.borrow_mut().ready = true;
    c.advance_wait_cycles(10);
    c.step();
    assert_eq!(c.instruction_count, 1);
    assert_eq!(c.get_r(Reg::R0), 0x40000008);
    assert_eq!(s.borrow().writes, [(0x40000000, 11), (0x40000004, 22)]);
}
#[test]
fn an_interrupt_can_preempt_a_deferred_instruction() {
    let (mut c, s) = fixture();
    c.step();
    c.write32(0xe000e100, 1).unwrap();
    c.write32(0xe000e200, 1).unwrap();
    c.step();
    c.step();
    assert_eq!(c.get_r(Reg::R3), 99);
    assert_eq!(s.borrow().writes.len(), 1);
    assert!(c.take_pending_fault_trap().is_none());
}
#[test]
fn instruction_reads_and_barriers_reach_the_attached_bus() {
    use zmu_cortex_m::core::instruction::Instruction;
    let (mut c, s) = fixture();
    c.set_pc(0x40000000);
    c.step();
    assert_eq!(c.get_r(Reg::R0), 1);
    for i in [Instruction::DMB, Instruction::DSB, Instruction::ISB] {
        c.execute(&i, 4);
    }
    assert_eq!(s.borrow().barriers, [0, 1, 2]);
}
