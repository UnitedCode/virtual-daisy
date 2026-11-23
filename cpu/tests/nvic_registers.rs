//! PM0253 4.2.2–4.2.5: clear registers report current state, just as set registers do.
use zmu_cortex_m::{Processor, bus::Bus};

#[test]
fn clear_enable_and_clear_pending_read_the_same_state_as_the_set_registers() {
    let mut cpu = Processor::new();
    use zmu_cortex_m::core::exception::{Exception, ExceptionState};
    for n in 0..160 {
        let e = Exception::Interrupt { n };
        cpu.exceptions
            .entry(e.into())
            .or_insert_with(|| ExceptionState::new(e, 0));
    }
    // Exercise independent banks, including the SAI and QSPI interrupt bank.
    for (bank, mask) in [(0, 1 << 11), (1, 1 << 15), (2, (1 << 23) | (1 << 28))] {
        let offset = bank * 4;
        let enable = 0xe000e100 + offset;
        let disable = 0xe000e180 + offset;
        let pending = 0xe000e200 + offset;
        let clear = 0xe000e280 + offset;
        assert_eq!(cpu.read32(disable).unwrap(), 0);
        assert_eq!(cpu.read32(clear).unwrap(), 0);
        cpu.write32(enable, mask).unwrap();
        cpu.write32(pending, mask).unwrap();
        assert_eq!(cpu.read32(disable).unwrap(), mask);
        assert_eq!(cpu.read32(clear).unwrap(), mask);
        cpu.write32(disable, 0).unwrap();
        cpu.write32(clear, 0).unwrap();
        assert_eq!(cpu.read32(enable).unwrap(), mask);
        assert_eq!(cpu.read32(pending).unwrap(), mask);
        cpu.write32(disable, mask).unwrap();
        assert_eq!(cpu.read32(enable).unwrap(), 0);
        assert_eq!(
            cpu.read32(pending).unwrap(),
            mask,
            "disable must retain pending"
        );
        cpu.write32(clear, mask).unwrap();
        assert_eq!(cpu.read32(clear).unwrap(), 0);
    }
}
