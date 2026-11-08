//! Electrically ideal keyboard-keyboard carrier fixture, not a thermal model.
//! Wiring follows the PCB netlist: PC0/PA3/PB1/PA7/PA6/PC1/PC4/PA5/PA4,
//! then PA1, PA0 and PA2 for the three inhibited mux pairs.
use crate::uart::Transmitter;
use std::{
    ffi::{c_char, CStr},
    fs,
};

pub struct Carrier {
    inputs: [[u32; 8]; 15],
    last_channel: Option<u32>,
    pub scans: u64,
    pub max_gap_ns: u64,
    pub last_scan_ns: Option<u64>,
    pub gaps_over_2ms: u64,
}
impl Default for Carrier {
    fn default() -> Self {
        Self {
            inputs: [[1_650_000; 8]; 15],
            last_channel: None,
            scans: 0,
            max_gap_ns: 0,
            last_scan_ns: None,
            gaps_over_2ms: 0,
        }
    }
}
impl Carrier {
    pub fn set_input(
        &mut self,
        mux: usize,
        channel: usize,
        microvolts: u32,
    ) -> Result<(), &'static str> {
        if mux >= 15 || channel >= 8 || microvolts > 3_300_000 {
            return Err("invalid carrier input");
        }
        self.inputs[mux][channel] = microvolts;
        Ok(())
    }
    pub fn sample(
        &mut self,
        adc: u32,
        pb: u32,
        pc: u32,
        pg: u32,
        now: u64,
    ) -> Result<u32, &'static str> {
        let channel = ((pg >> 10) & 1) | (((pg >> 11) & 1) << 1) | (((pb >> 4) & 1) << 2);
        let decoder = ((pc >> 8) & 1) | (((pc >> 9) & 1) << 1) | (((pc >> 10) & 1) << 2);
        let mux = match adc {
            10 => 0,
            15 => 1,
            5 => 2,
            7 => 3,
            3 => 4,
            11 => 5,
            4 => 6,
            19 => 7,
            18 => 8,
            17 if decoder < 2 => 9 + decoder,
            16 if (2..4).contains(&decoder) => 9 + decoder,
            14 if (4..6).contains(&decoder) => 9 + decoder,
            _ => return Err("ADC pin has no actively enabled carrier mux"),
        };
        if adc == 10 {
            if channel == 0 && self.last_channel == Some(7) {
                if let Some(last) = self.last_scan_ns {
                    let gap = now.saturating_sub(last);
                    self.max_gap_ns = self.max_gap_ns.max(gap);
                    self.gaps_over_2ms += u64::from(gap > 2_000_000);
                }
                self.last_scan_ns = Some(now);
                self.scans += 1;
            }
            self.last_channel = Some(channel);
        }
        if mux == 14 && channel < 4 {
            return Ok(0);
        }
        Ok(self.inputs[mux as usize][channel as usize])
    }
}

pub struct Session {
    carrier: Carrier,
    tx: Transmitter,
    registers: [u32; 12],
    pub(crate) instantaneous: bool,
    bytes: Vec<(u64, u8)>,
}
impl Session {
    fn collect(&mut self, now: u64) {
        self.tx.ready(now);
        self.bytes.extend(self.tx.take_completed());
    }
    fn uart(
        &mut self,
        offset: u32,
        value: u32,
        write: bool,
        now: u64,
        hz: u64,
    ) -> Result<u32, &'static str> {
        self.collect(now);
        if !offset.is_multiple_of(4) || offset > 0x2c {
            return Err("unsupported keyboard USART register");
        }
        if write {
            if offset == 0x28 {
                let cr1 = self.registers[0];
                if cr1 & 9 != 9
                    || cr1 & (1 << 29) == 0
                    || cr1 & ((1 << 12) | (1 << 28) | (1 << 10) | (1 << 15)) != 0
                    || self.registers[1] & 0x3000 != 0
                {
                    return Err("keyboard UART requires enabled FIFO, 8N1 oversampling16");
                }
                if self.instantaneous {
                    self.bytes.push((now, value as u8));
                } else {
                    self.tx.configure(hz, self.registers[3])?;
                    self.tx.write(value as u8, now)?;
                }
                return Ok(0);
            }
            if offset == 0 && value & ((1 << 6) | (1 << 7)) != 0 {
                return Err("TX interrupts are outside the polling UART scenario");
            }
            if offset == 0x2c && value != 0 {
                return Err("UART prescaler other than divide-by-one is unsupported");
            }
            self.registers[offset as usize / 4] = value;
            return Ok(0);
        }
        if offset == 0x1c {
            return Ok((u32::from(self.instantaneous || self.tx.ready(now)) << 7)
                | (u32::from(self.instantaneous || self.tx.complete(now)) << 6)
                | ((self.registers[0] & (1 << 3)) << 18)
                | ((self.registers[0] & (1 << 2)) << 20));
        }
        Ok(self.registers[offset as usize / 4])
    }
}

/// Reject runtime defects rather than mistaking dropped MMIO writes for speed.
/// The extra ADC warnings are scoped to a one-channel, software-triggered
/// discontinuous conversion; analog settling and I2C wire timing remain unmodeled.
pub fn validate_log(log: &str) -> Result<(), String> {
    for line in log
        .lines()
        .filter(|s| s.contains("[WARNING]") || s.contains("[ERROR]"))
    {
        let Some((_, message)) = line.split_once("[WARNING] ") else {
            return Err(line.into());
        };
        let message = message.strip_prefix("[no-name]: ").unwrap_or(message);
        let message = match message.rsplit_once(" (") {
            Some((base, count))
                if count
                    .strip_suffix(')')
                    .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())) =>
            {
                base
            }
            _ => message,
        };
        let allowed = include_str!("../known-warnings.txt").lines()
            .filter(|s| !s.starts_with("usart1:") && !s.is_empty())
            .any(|known| known == message)
            || [
                "Unhandled write to offset 0xC. Unhandled bits: [16] when writing value 0x10000. Tags: DISCEN (0x1).",
                "Unhandled write to offset 0xC. Unhandled bits: [16] when writing value 0x10008. Tags: DISCEN (0x1).",
            ].contains(&message);
        if !allowed {
            return Err(line.into());
        }
    }
    Ok(())
}
// All exported state is owned/serialized by the Renode adapter. Errors cross
// the boundary as sentinel values; no guest state is edited by these helpers.
#[no_mangle]
pub extern "C" fn daisy_keyboard_new(instant: u32) -> *mut Session {
    Box::into_raw(Box::new(Session {
        carrier: Carrier::default(),
        tx: Transmitter::new(16),
        registers: [0; 12],
        instantaneous: instant != 0,
        bytes: Vec::new(),
    }))
}
/// # Safety
/// The pointer must be a live, exclusively accessed session from `daisy_keyboard_new`.
#[no_mangle]
pub unsafe extern "C" fn daisy_keyboard_input(s: *mut Session, mux: u32, ch: u32, uv: u32) -> u32 {
    u32::from(
        (*s).carrier
            .set_input(mux as usize, ch as usize, uv)
            .is_err(),
    )
}
/// # Safety
/// The pointer must be a live, exclusively accessed session.
#[no_mangle]
pub unsafe extern "C" fn daisy_keyboard_sample(
    s: *mut Session,
    adc: u32,
    pb: u32,
    pc: u32,
    pg: u32,
    now: u64,
) -> u32 {
    (*s).carrier
        .sample(adc, pb, pc, pg, now)
        .unwrap_or(u32::MAX)
}
/// # Safety
/// The pointer must be a live, exclusively accessed session.
#[no_mangle]
pub unsafe extern "C" fn daisy_keyboard_uart(
    s: *mut Session,
    off: u32,
    v: u32,
    write: u32,
    now: u64,
    hz: u64,
) -> u64 {
    (*s).uart(off, v, write != 0, now, hz)
        .map(u64::from)
        .unwrap_or(u64::MAX)
}
/// # Safety
/// Session must be live; path must be a valid, terminated UTF-8 C string.
#[no_mangle]
pub unsafe extern "C" fn daisy_keyboard_save(
    s: *mut Session,
    path: *const c_char,
    now: u64,
) -> u32 {
    let session = &mut *s;
    session.collect(now);
    let Ok(path) = CStr::from_ptr(path).to_str() else {
        return 1;
    };
    let c = &session.carrier;
    let stats=format!("elapsed_ns={now}\nscans={}\nmax_scan_gap_ns={}\nscan_gaps_over_2ms={}\nmidi_bytes={}\ninstant_uart={}\nhardware_equivalence=UNVERIFIED\nthermal_and_power_behavior=UNMODELED\n",c.scans,c.max_gap_ns,c.gaps_over_2ms,session.bytes.len(),session.instantaneous);
    let mut bytes = String::from("completed_ns,byte\n");
    for (t, b) in &session.bytes {
        bytes += &format!("{t},{b}\n");
    }
    u32::from(
        fs::write(format!("{path}.keyboard.txt"), stats)
            .and_then(|_| fs::write(format!("{path}.midi.csv"), bytes))
            .is_err(),
    )
}
/// # Safety
/// The pointer must be a live session and must not be used again after this call.
#[no_mangle]
pub unsafe extern "C" fn daisy_keyboard_free(s: *mut Session) {
    drop(Box::from_raw(s));
}

/// # Safety
/// Session must be live and exclusively accessed. An RCC peripheral reset
/// clears UART state, preserving external voltages and the experiment mode.
#[no_mangle]
pub unsafe extern "C" fn daisy_keyboard_reset_uart(s: *mut Session, now: u64) {
    let s = &mut *s;
    s.collect(now);
    s.tx = Transmitter::new(16);
    s.registers = [0; 12];
}

/// # Safety
/// Session must be live and exclusively accessed. Only host observations reset.
#[no_mangle]
pub unsafe extern "C" fn daisy_keyboard_clear(s: *mut Session, now: u64) {
    let s = &mut *s;
    s.collect(now);
    s.bytes.clear();
    s.carrier.scans = 0;
    s.carrier.max_gap_ns = 0;
    s.carrier.gaps_over_2ms = 0;
    s.carrier.last_scan_ns = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rcc_uart_reset_preserves_carrier_inputs_and_instant_control() {
        unsafe {
            let session = daisy_keyboard_new(1);
            assert_eq!(daisy_keyboard_input(session, 0, 0, 2_200_000), 0);
            daisy_keyboard_reset_uart(session, 0);
            assert!((*session).instantaneous);
            assert_eq!(daisy_keyboard_sample(session, 10, 0, 0, 0, 1), 2_200_000);
            daisy_keyboard_free(session);
        }
    }
}
