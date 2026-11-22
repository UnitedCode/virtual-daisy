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
impl Firmware {
    #[inline(always)]
    fn single_translation_enabled(&self) -> bool {
        #[cfg(feature = "profile")]
        {
            !self.profile_fallback_only
        }
        #[cfg(not(feature = "profile"))]
        {
            true
        }
    }
    fn service(&mut self) -> Result<(), String> {
        let mut board = self.board.borrow_mut();
        board.synchronize(self.cpu.cycle_count)?;
        self.cpu.take_device_boundary();
        if let Some(error) = board.error.take() {
            return Err(format!(
                "{error}; PC=0x{:08x}, instructions={}",
                self.cpu.get_pc(),
                self.cpu.instruction_count
            ));
        }
        let irqs = board.irqs();
        use zmu_cortex_m::core::exception::{Exception, ExceptionHandling};
        for (stream, n) in [11, 12, 13, 14, 15, 16, 17, 47].iter().enumerate() {
            let enabled = self.cpu.nvic_interrupt_enabled[n / 32] & (1 << (n % 32)) != 0;
            let e = Exception::Interrupt { n: *n };
            if irqs & (1 << stream) != 0 && enabled && !self.cpu.exception_active(e) {
                self.cpu.set_exception_pending(e);
            }
        }
        for (asserted, n) in [(board.uart_irq(), 37usize), (board.timer_irq(), 28)] {
            let e = Exception::Interrupt { n };
            if asserted
                && self.cpu.nvic_interrupt_enabled[n / 32] & (1 << (n % 32)) != 0
                && !self.cpu.exception_active(e)
            {
                self.cpu.set_exception_pending(e);
            }
        }
        self.next_service_cycle =
            self.cpu.cycle_count + board.next_service_cycles(self.maximum_service_ns)?;
        Ok(())
    }
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
    next_service_cycle: u64,
    maximum_service_ns: u64,
    slice_deadline: u64,
    idle_pc: Option<u32>,
    delay_pc: Vec<u32>,
    translated: bool,
    #[cfg(feature = "profile")]
    profile: Vec<u64>,
    #[cfg(feature = "profile")]
    profile_fallback_only: bool,
}
#[wasm_bindgen]
impl Firmware {
    pub fn flash_image(&self) -> Vec<u8> {
        self.board.borrow().flash_image()
    }
    pub fn flash_revision(&self) -> u64 {
        self.board.borrow().flash_revision()
    }
    pub fn restore_flash(&mut self, bytes: &[u8]) -> Result<(), String> {
        if self.cpu.instruction_count != 0 {
            return Err("Restore flash before booting the CPU".into());
        }
        self.board
            .borrow_mut()
            .restore_flash(bytes)
            .map_err(String::from)
    }
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
            next_service_cycle: 0,
            maximum_service_ns: 100_000,
            slice_deadline: u64::MAX,
            idle_pc,
            delay_pc,
            translated,
            #[cfg(feature = "profile")]
            profile: vec![0; 65536],
            #[cfg(feature = "profile")]
            profile_fallback_only: false,
        })
    }
    pub fn run(&mut self, steps: u32) -> Result<Vec<u32>, String> {
        if self.next_service_cycle == 0 {
            self.service()?;
        }
        let singles = self.single_translation_enabled();
        // Streaming callers already have a time deadline. Chain more decoded
        // blocks within that deadline; retain the lab's bounded step behavior.
        let translated_budget = if self.slice_deadline == u64::MAX {
            256
        } else {
            4096
        };
        for _ in 0..steps.min(10_000_000) {
            let pc = self.cpu.get_r(Reg::PC);
            let budget = self
                .next_service_cycle
                .saturating_sub(self.cpu.cycle_count)
                .min(u64::from(u32::MAX)) as u32;
            #[cfg(feature = "profile")]
            if !self.profile_fallback_only {
                if let Some(count) = self
                    .profile
                    .get_mut(pc.wrapping_sub(0x08000004) as usize / 2)
                {
                    *count += 1;
                }
            }
            if self.idle_pc == Some(self.cpu.get_pc()) && self.cpu.advance_nop_call_loop(budget) {
            } else if self.delay_pc.contains(&self.cpu.get_pc())
                && self.cpu.advance_decrement_loop(budget)
            {
            } else if self.translated
                && self.next_service_cycle.saturating_sub(self.cpu.cycle_count) >= 64
                && compiled::contains(pc - 4)
                && compiled::dispatch(
                    &mut self.cpu,
                    pc - 4,
                    u64::from(budget.min(translated_budget)),
                )
            {
            } else if self.cpu.sleeping {
                self.cpu.step_sleep();
            } else if self.translated && singles && compiled::single(&mut self.cpu, pc - 4) {
            } else {
                #[cfg(feature = "profile")]
                if self.profile_fallback_only {
                    if let Some(count) = self
                        .profile
                        .get_mut((pc - 4).wrapping_sub(0x08000000) as usize / 2)
                    {
                        *count += 1;
                    }
                }
                self.cpu.step();
            }
            if let Some(error) = self.cpu.take_pending_fault_trap() {
                return Err(format!(
                    "{error}; MMFAR={:08x} BFAR={:08x} CFSR={:08x} r0-r12={:08x?} SP={:08x}",
                    self.cpu.mmfar,
                    self.cpu.bfar,
                    self.cpu.cfsr,
                    self.cpu.r0_12,
                    self.cpu.get_r(Reg::SP)
                ));
            }
            if self.cpu.take_device_boundary() || self.cpu.cycle_count >= self.next_service_cycle {
                self.service()?;
                if self.board.borrow().now_ns >= self.slice_deadline {
                    break;
                }
            }
        }
        // Host controls and persisted flash observe the end of this CPU slice.
        self.service()?;
        Ok(vec![
            self.cpu.get_r(Reg::PC),
            self.cpu.instruction_count as u32,
            self.cpu.cycle_count as u32,
        ])
    }
    /// Bound a browser slice by both CPU work and elapsed board time.
    /// This keeps a fast idle loop from exhausting captured microphone frames.
    pub fn run_for(&mut self, steps: u32, nanoseconds: u64) -> Result<Vec<u32>, String> {
        let deadline = self.virtual_time_ns().saturating_add(nanoseconds);
        let previous = std::mem::replace(&mut self.slice_deadline, deadline);
        let result = self.run(steps);
        self.slice_deadline = previous;
        result
    }
    pub fn midi(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.board
            .borrow_mut()
            .receive_midi(bytes)
            .map_err(String::from)
    }
    pub fn input(&mut self, samples: &[f32]) {
        let mut board = self.board.borrow_mut();
        for &sample in samples.iter().take(96_028) {
            if board.input.len() >= 96_028 {
                board.input.pop_front();
            }
            let sample = if sample.is_finite() {
                sample.clamp(-1., 1.)
            } else {
                0.
            };
            board.input.push_back([sample, sample]);
        }
    }
    /// Frames waiting at the codec input, for bounded browser streaming.
    pub fn pending_input(&self) -> usize {
        self.board.borrow().input.len()
    }
    /// Stereo codec input: left external input, right telephone microphone.
    pub fn input_stereo(&mut self, samples: &[f32]) -> Result<(), String> {
        if samples.len() % 2 != 0 {
            return Err("Stereo input needs pairs of samples".into());
        }
        let mut board = self.board.borrow_mut();
        for pair in samples.chunks_exact(2).take(96_028) {
            if board.input.len() >= 96_028 {
                board.input.pop_front();
            }
            board.input.push_back([pair[0], pair[1]].map(|s| {
                if s.is_finite() {
                    s.clamp(-1., 1.)
                } else {
                    0.
                }
            }));
        }
        Ok(())
    }
    /// Physical codec clock, including the original firmware's PLL mismatch.
    pub fn sample_rate(&self) -> f64 {
        self.board.borrow_mut().sample_rate().unwrap_or(0.)
    }
    pub fn stats(&self) -> String {
        use zmu_cortex_m::core::register::Ipsr;
        let mut b = self.board.borrow_mut();
        let audio = b.audio_status();
        format!(
            "ns={} frames={} dma_beats={} reads={} instructions={} lifted={} AOT={} ISR={} LR={:08x} {audio}",
            b.now_ns,
            b.frames,
            b.dma_beats,
            b.reads,
            self.cpu.instruction_count,
            self.cpu.lifted_instructions,
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

#[cfg(feature = "profile")]
impl Firmware {
    pub fn start_profile(&mut self) {
        self.translated = false;
        self.profile_fallback_only = false;
        self.profile.fill(0);
    }
    pub fn start_fallback_profile(&mut self) {
        self.profile_fallback_only = true;
        self.profile.fill(0);
    }
    pub fn profile_counts(&self) -> &[u64] {
        &self.profile
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
    fn browser_slices_stop_before_consuming_future_microphone_audio() {
        let mut f = Firmware::new(reference_image()).unwrap();
        for _ in 0..8 {
            let before = f.virtual_time_ns();
            f.run_for(1_000_000, 2_000_000).unwrap();
            let elapsed = f.virtual_time_ns() - before;
            assert!(
                (2_000_000..2_101_000).contains(&elapsed),
                "unbounded browser slice: {elapsed} ns"
            );
        }
    }
    #[test]
    fn scheduled_audio_agrees_with_fine_clock_service() {
        fn capture(maximum_ns: u64) -> Vec<f32> {
            let mut f = Firmware::new(reference_image()).unwrap();
            f.maximum_service_ns = maximum_ns;
            f.input(
                &(0..12_000)
                    .map(|n| (n as f32 * 0.0287).sin() * 0.16)
                    .collect::<Vec<_>>(),
            );
            let mut audio = Vec::new();
            while audio.len() < 12_000 {
                f.run(50_000).unwrap();
                audio.extend(f.take_audio());
            }
            audio.truncate(12_000);
            audio
        }
        let scheduled = capture(100_000);
        let fine = capture(1_000);
        assert!(scheduled.iter().any(|sample| sample.abs() > 0.01));
        let error = scheduled
            .iter()
            .zip(&fine)
            .map(|(a, b)| (a - b).abs())
            .sum::<f32>()
            / scheduled.len() as f32;
        assert!(
            error < 0.0001,
            "board service changed firmware PCM: {error}"
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
    fn advance(f: &mut Firmware, milliseconds: u64) {
        let deadline = f.virtual_time_ns() + milliseconds * 1_000_000;
        while f.virtual_time_ns() < deadline {
            f.run(100_000).unwrap();
            f.take_audio();
        }
    }
    #[test]
    fn all_six_original_profiles_stream_finite_audio() {
        let mut f = Firmware::new(reference_image()).unwrap();
        advance(&mut f, 500);
        f.input(
            &(0..96028)
                .map(|n| (n as f32 * 220. * std::f32::consts::TAU / 48_187.487).sin() * 0.16)
                .collect::<Vec<_>>(),
        );
        f.midi(&[
            0x90, 69, 100, 0x91, 60, 100, 64, 90, 67, 90, 0xb0, 80, 127, 81, 127,
        ])
        .unwrap();
        let mut displays = std::collections::BTreeSet::new();
        for mode in 0..6 {
            if mode == 5 {
                f.midi(&[0x99, 36, 100]).unwrap();
            }
            let end = f.virtual_time_ns() + 240_000_000;
            let mut samples = 0;
            while f.virtual_time_ns() < end {
                f.run(100_000).unwrap();
                let audio = f.take_audio();
                assert!(
                    audio.iter().all(|sample| sample.is_finite()),
                    "profile {mode} emitted invalid PCM"
                );
                samples += audio.len();
            }
            assert!(samples > 4800, "profile {mode} stalled the codec");
            displays.insert(f.oled());
            if mode < 5 {
                f.key(10, true).unwrap();
                advance(&mut f, 80);
                f.key(10, false).unwrap();
                advance(&mut f, 80);
            }
        }
        assert_eq!(
            displays.len(),
            6,
            "the physical controls must reach six distinct firmware profiles"
        );
    }
    #[test]
    fn encoder_button_and_rotation_reach_the_original_menu() {
        let mut f = Firmware::new(reference_image()).unwrap();
        advance(&mut f, 500);
        let before = f.oled();
        f.encoder_button(true).unwrap();
        advance(&mut f, 40);
        f.encoder_button(false).unwrap();
        advance(&mut f, 40);
        assert_ne!(
            f.oled(),
            before,
            "encoder press/release must change the firmware screen"
        );
        f.encoder_button(true).unwrap();
        advance(&mut f, 40);
        f.encoder_button(false).unwrap();
        advance(&mut f, 40);
        let menu = f.oled();
        f.encoder(-2);
        advance(&mut f, 80);
        assert_ne!(f.oled(), menu, "quadrature must change menu selection");
    }
    #[test]
    fn streamed_mic_and_hook_changes_do_not_corrupt_firmware_state() {
        let mut f = Firmware::new(reference_image()).unwrap();
        let flash = f.flash_image();
        f.restore_flash(&flash).unwrap();
        f.handset(false).unwrap();
        let mut phase = 0f64;
        while f.virtual_time_ns() < 600_000_000 {
            let t = f.virtual_time_ns();
            f.handset(t > 120_000_000).unwrap();
            if f.pending_input() < 4096 {
                let input: Vec<f32> = (0..4096)
                    .flat_map(|_| {
                        let sample = if t > 90_000_000 {
                            (phase * std::f64::consts::TAU).sin() as f32 * 0.16
                        } else {
                            0.
                        };
                        phase = (phase + 220. / 48_187.487) % 1.;
                        [0., sample]
                    })
                    .collect();
                f.input_stereo(&input).unwrap();
            }
            f.run(10_000).unwrap();
            f.take_audio();
        }
    }
    #[test]
    fn handset_switch_selects_the_actual_codec_channel() {
        for lifted in [false, true] {
            let mut f = Firmware::new(reference_image()).unwrap();
            f.handset(lifted).unwrap();
            advance(&mut f, 40);
            let samples: Vec<f32> = (0..20000)
                .flat_map(|n| {
                    [
                        0.,
                        (n as f32 * 220. * std::f32::consts::TAU / f.sample_rate() as f32).sin()
                            * 0.2,
                    ]
                })
                .collect();
            f.input_stereo(&samples).unwrap();
            let mut out = Vec::new();
            while out.len() < 12000 {
                f.run(100_000).unwrap();
                out.extend(f.take_audio());
            }
            let energy = out[4000..].iter().map(|s| s * s).sum::<f32>();
            if lifted {
                assert!(energy > 10.);
            } else {
                assert_eq!(energy, 0.);
            }
        }
    }
    #[test]
    fn firmware_settings_survive_a_virtual_power_cycle() {
        let mut f = Firmware::new(reference_image()).unwrap();
        advance(&mut f, 600);
        let original = f.flash_image();
        f.encoder(-2);
        advance(&mut f, 2600);
        let saved = f.flash_image();
        assert_ne!(
            saved, original,
            "firmware must commit an encoder setting through QSPI"
        );
        let screen = f.oled();
        let mut restored = Firmware::new(reference_image()).unwrap();
        restored.restore_flash(&saved).unwrap();
        advance(&mut restored, 600);
        assert!(
            restored.oled() == screen,
            "boot must restore the original firmware settings"
        );
        assert!(
            restored.restore_flash(&saved).is_err(),
            "cannot swap live flash under a booted CPU"
        );
    }
}
