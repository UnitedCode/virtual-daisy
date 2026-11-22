//! Rust Cortex-M7 execution backend. Runs ARM bytes, not a recompiled instrument.
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::prelude::*;
use zmu_cortex_m::{
    bus::Bus,
    core::{
        register::{BaseReg, Reg},
        reset::Reset,
    },
    executor::Executor,
    memory::map::MemoryMapConfig,
    Processor,
};
mod adc;
mod board;
mod i2c;
mod timer;
mod uart;
mod compiled {
    include!(concat!(env!("OUT_DIR"), "/compiled.rs"));
}
/// Original, unmodified ARM image for consumers building the Synthphone UI.
pub fn reference_image() -> &'static [u8] {
    include_bytes!("../../firmware/synthphone.bin")
}
#[cfg(all(target_arch = "wasm32", feature = "lab"))]
mod browser;

#[wasm_bindgen]
pub struct Firmware {
    cpu: Processor,
    board: Rc<RefCell<board::Board>>,
    pending_cycles: u64,
    service_cycles: u64,
    idle_pc: Option<u32>,
    delay_pc: Vec<u32>,
    translated: bool,
}
#[wasm_bindgen]
impl Firmware {
    #[wasm_bindgen(constructor)]
    pub fn new(image: &[u8]) -> Result<Firmware, String> {
        if image.len() < 8 || image.len() > 128 * 1024 {
            return Err("Expected a raw Daisy flash image, 8..131072 bytes".into());
        }
        let board = Rc::new(RefCell::new(board::Board::new()));
        let mut cpu = Processor::new();
        cpu.fault_trap_mode(zmu_cortex_m::core::fault::FaultTrapMode::all());
        let mut flash = vec![0xff; 128 * 1024];
        flash[..image.len()].copy_from_slice(image);
        cpu.flash_memory(flash.len(), &flash);
        cpu.memory_map(Some(MemoryMapConfig::new(0x08000000, 0, 128 * 1024)));
        cpu.device(Some(Box::new(board::Device(board.clone()))));
        cpu.cpuid = 0x411fc271;
        cpu.ccr = 1 << 9;
        // STM32H750 exposes 150 external interrupt slots, beyond the generic
        // interpreter's default 32. Populate the board's complete vector space.
        for n in 0..160 {
            let irq = zmu_cortex_m::core::exception::Exception::Interrupt { n };
            cpu.exceptions
                .entry(usize::from(irq))
                .or_insert_with(|| zmu_cortex_m::core::exception::ExceptionState::new(irq, 0));
        }
        cpu.ictr = 4;
        cpu.vtor = 0x08000000;
        cpu.cache_instructions();
        cpu.reset().map_err(|e| format!("{e:?}"))?;
        use zmu_cortex_m::{
            core::instruction::Instruction, core::thumb::ThumbCode, decoder::Decoder,
        };
        let idle_pc = (0..image.len().saturating_sub(6))
            .step_by(2)
            .find_map(|offset| {
                if image[offset + 4..offset + 6] != [0xfc, 0xe7] {
                    return None;
                }
                let opcode = (u32::from(u16::from_le_bytes([image[offset], image[offset + 1]]))
                    << 16)
                    | u32::from(u16::from_le_bytes([image[offset + 2], image[offset + 3]]));
                if opcode & 0xf800d000 != 0xf000d000 {
                    return None;
                }
                let Instruction::BL { imm32 } = cpu.decode(ThumbCode::Thumb32 { opcode }) else {
                    return None;
                };
                let target = (offset as i32 + 4 + imm32) as usize;
                (image.get(target..target + 4) == Some(&[0x00, 0xbf, 0x70, 0x47]))
                    .then_some(0x08000000 + offset as u32)
            });
        let delay_pc = (0..image.len().saturating_sub(4))
            .step_by(2)
            .filter(|&n| image[n..n + 4] == [1, 0x38, 0xfd, 0xd1])
            .map(|n| 0x08000000 + n as u32)
            .collect();
        use sha2::{Digest, Sha256};
        let translated = Sha256::digest(image).as_slice() == compiled::IMAGE_HASH;
        Ok(Self {
            cpu,
            board,
            pending_cycles: 0,
            service_cycles: 64,
            idle_pc,
            delay_pc,
            translated,
        })
    }
    pub fn run(&mut self, steps: u32) -> Result<Vec<u32>, String> {
        for _ in 0..steps.min(10_000_000) {
            let before = self.cpu.cycle_count;
            let pc = self.cpu.get_r(Reg::PC);
            if self.idle_pc == Some(self.cpu.get_pc()) && self.cpu.advance_nop_call_loop(440) {
            } else if self.delay_pc.contains(&self.cpu.get_pc())
                && self.cpu.advance_decrement_loop(440)
            {
            } else if self.translated
                && compiled::contains(pc - 4)
                && compiled::dispatch(&mut self.cpu, pc - 4)
            {
            } else if self.cpu.sleeping {
                self.cpu.step_sleep();
            } else {
                self.cpu.step();
            }
            if let Some(error) = self.cpu.take_pending_fault_trap() {
                return Err(format!(
                    "{error}; MMFAR={:08x} BFAR={:08x} CFSR={:08x}",
                    self.cpu.mmfar, self.cpu.bfar, self.cpu.cfsr
                ));
            }
            self.pending_cycles += self.cpu.cycle_count - before;
            if self.pending_cycles < self.service_cycles {
                continue;
            }
            self.board
                .borrow_mut()
                .advance(std::mem::take(&mut self.pending_cycles))?;
            self.service_cycles = (self.board.borrow().core_rate / 1_000_000.0).max(1.0) as u64;
            if let Some(error) = self.board.borrow_mut().error.take() {
                return Err(format!(
                    "{error}; PC=0x{pc:08x}, instructions={}",
                    self.cpu.instruction_count
                ));
            }
            let irqs = self.board.borrow().irqs();
            use zmu_cortex_m::core::exception::{Exception, ExceptionHandling};
            for (stream, n) in [11, 12, 13, 14, 15, 16, 17, 47].iter().enumerate() {
                let enabled = self.cpu.nvic_interrupt_enabled[n / 32] & (1 << (n % 32)) != 0;
                let e = Exception::Interrupt { n: *n };
                if irqs & (1 << stream) != 0 && enabled && !self.cpu.exception_active(e) {
                    self.cpu.set_exception_pending(e);
                }
            }
            if self.board.borrow().uart_irq() && self.cpu.nvic_interrupt_enabled[1] & (1 << 5) != 0
            {
                let e = Exception::Interrupt { n: 37 };
                if !self.cpu.exception_active(e) {
                    self.cpu.set_exception_pending(e);
                }
            }
            if self.board.borrow().timer_irq()
                && self.cpu.nvic_interrupt_enabled[0] & (1 << 28) != 0
            {
                let e = Exception::Interrupt { n: 28 };
                if !self.cpu.exception_active(e) {
                    self.cpu.set_exception_pending(e);
                }
            }
        }
        Ok(vec![
            self.cpu.get_r(Reg::PC),
            self.cpu.instruction_count as u32,
            self.cpu.cycle_count as u32,
        ])
    }
    pub fn midi(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.board
            .borrow_mut()
            .receive_midi(bytes)
            .map_err(String::from)
    }
    pub fn input(&mut self, samples: &[f32]) {
        self.board
            .borrow_mut()
            .input
            .extend(samples.iter().map(|&s| [s, s]));
    }
    pub fn stats(&self) -> String {
        use zmu_cortex_m::core::register::Ipsr;
        let mut b = self.board.borrow_mut();
        let audio = b.audio_status();
        format!(
            "ns={} frames={} dma_beats={} reads={} instructions={} AOT={} ISR={} LR={:08x} {audio}",
            b.now_ns,
            b.frames,
            b.dma_beats,
            b.reads,
            self.cpu.instruction_count,
            self.translated,
            self.cpu.psr.get_isr_number(),
            self.cpu.lr
        )
    }
    pub fn take_audio(&mut self) -> Vec<f32> {
        self.board.borrow_mut().take_audio()
    }
    pub fn oled(&self) -> Vec<u8> {
        self.board.borrow().oled()
    }
    /// Elapsed time on the modeled board, independent of browser wall time.
    pub fn virtual_time_ns(&self) -> u64 {
        self.board.borrow().now_ns
    }
    pub fn key(&mut self, key: u8, pressed: bool) -> Result<(), String> {
        self.board
            .borrow_mut()
            .key(key, pressed)
            .map_err(String::from)
    }
    pub fn encoder(&mut self, steps: i32) {
        self.board.borrow_mut().turn(steps);
    }
    pub fn encoder_button(&mut self, pressed: bool) -> Result<(), String> {
        self.board
            .borrow_mut()
            .switch(10, !pressed)
            .map_err(String::from)
    }
    pub fn handset(&mut self, lifted: bool) -> Result<(), String> {
        self.board
            .borrow_mut()
            .switch(11, lifted)
            .map_err(String::from)
    }
    pub fn mic_gain(&mut self, value: f32) -> Result<(), String> {
        if !value.is_finite() || !(0.0..=4.0).contains(&value) {
            return Err("Mic gain must be between 0 and 4".into());
        }
        self.board.borrow_mut().mic_gain = value;
        Ok(())
    }
}

#[wasm_bindgen]
pub fn cpu_probe(steps: u32) -> Result<Vec<u32>, String> {
    // Vector table, then MOVS r0,#1; ADDS r0,#1; B -6.
    let mut image = vec![0; 32];
    image[..4].copy_from_slice(&0x2001fff0u32.to_le_bytes());
    image[4..8].copy_from_slice(&9u32.to_le_bytes());
    image[8..14].copy_from_slice(&[0x01, 0x20, 0x01, 0x30, 0xfd, 0xe7]);
    let mut cpu = Processor::new();
    cpu.flash_memory(image.len(), &image);
    cpu.cache_instructions();
    cpu.reset().map_err(|e| format!("{e:?}"))?;
    for _ in 0..steps.min(10_000_000) {
        cpu.step();
        if let Some(e) = cpu.take_pending_fault_trap() {
            return Err(format!("{e}"));
        }
    }
    let _ = cpu.read32(0);
    Ok(vec![
        cpu.r0_12[0],
        cpu.get_r(Reg::PC),
        cpu.instruction_count as u32,
    ])
}

#[wasm_bindgen]
pub fn floating_probe(steps: u32) -> Result<Vec<u32>, String> {
    let mut image = vec![0; 32];
    image[..4].copy_from_slice(&0x2001fff0u32.to_le_bytes());
    image[4..8].copy_from_slice(&9u32.to_le_bytes());
    // VMUL.F32 s0,s0,s28; B 0x8. Opcode taken from the real firmware.
    image[8..14].copy_from_slice(&[0x20, 0xee, 0x0e, 0x0a, 0xfc, 0xe7]);
    let mut cpu = Processor::new();
    cpu.flash_memory(image.len(), &image);
    cpu.reset().map_err(|e| e.to_string())?;
    cpu.cpacr = 0x00f00000;
    cpu.fp_regs[0] = 1.0f32.to_bits();
    cpu.fp_regs[28] = 1.000001f32.to_bits();
    for _ in 0..steps.min(10_000_000) {
        cpu.step();
        if let Some(e) = cpu.take_pending_fault_trap() {
            return Err(e.to_string());
        }
    }
    Ok(vec![cpu.fp_regs[0], cpu.instruction_count as u32])
}

#[cfg(test)]
mod tests {
    use super::*;
    fn render(mic: bool, channel: Option<u8>, translated: bool) -> Vec<f32> {
        let mut f = Firmware::new(include_bytes!("../../firmware/synthphone.bin")).unwrap();
        f.translated = translated;
        f.input(
            &(0..12_000)
                .map(|n| {
                    if mic {
                        (n as f32 * 220. * std::f32::consts::TAU / 48_014.312).sin() * 0.2
                    } else {
                        0.
                    }
                })
                .collect::<Vec<_>>(),
        );
        f.run(2_000_000).unwrap();
        if let Some(channel) = channel {
            f.midi(&[0x90 | channel, 69, 100]).unwrap();
        }
        let mut out = Vec::new();
        for _ in 0..1000 {
            f.run(100_000).unwrap();
            out.extend(f.take_audio());
            if out.len() >= 6_000 {
                break;
            }
        }
        assert!(
            out.len() >= 6000,
            "firmware failed to make audio progress: {}",
            f.stats()
        );
        assert!(out.iter().all(|s| s.is_finite() && s.abs() <= 1.));
        out
    }
    #[test]
    fn unchanged_firmware_processes_mic_and_preserves_silence() {
        let silent = render(false, None, true);
        assert!(silent.iter().all(|s| *s == 0.));
        let out = render(true, None, true);
        let energy = out.iter().map(|s| s * s).sum::<f32>() / out.len() as f32;
        assert!(
            energy > 0.005 && energy < 0.1,
            "unexpected tone energy {energy}"
        );
    }
    #[test]
    fn actual_midi_routing_separates_vocal_control_and_synth() {
        assert!(render(false, Some(0), true).iter().all(|s| *s == 0.));
        assert!(render(false, Some(1), true).iter().any(|s| s.abs() > 0.05));
    }
    #[test]
    fn translated_arm_blocks_agree_with_interpreter_audio() {
        let a = render(true, None, true);
        let b = render(true, None, false);
        let n = a.len().min(b.len());
        let mean_error = a[..n]
            .iter()
            .zip(&b[..n])
            .map(|(a, b)| (a - b).abs())
            .sum::<f32>()
            / n as f32;
        assert!(
            mean_error < 0.0001,
            "AOT/interpreter audio mismatch {mean_error}"
        );
    }
    #[test]
    fn physical_keypad_scan_drives_the_firmware_screen() {
        let mut f = Firmware::new(include_bytes!("../../firmware/synthphone.bin")).unwrap();
        f.run(3_000_000).unwrap();
        let before = f.oled();
        f.key(10, true).unwrap();
        let deadline = f.board.borrow().now_ns + 40_000_000;
        for _ in 0..200 {
            f.run(100_000).unwrap();
            f.take_audio();
            if f.board.borrow().now_ns >= deadline {
                break;
            }
        }
        f.key(10, false).unwrap();
        assert_ne!(
            f.oled(),
            before,
            "keypad zero must change the real firmware profile display"
        );
    }
}
