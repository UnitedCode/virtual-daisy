//! Architecturally visible PSR fields; EPSR is read-as-zero through MRS.
const APSR: u32 = 0xf8000000
    | if cfg!(feature = "has-dsp-ext") {
        0x000f0000
    } else {
        0
    };
/// Read the MRS alias selected by SYSm: APSR (0), IAPSR (1), EAPSR (2),
/// XPSR (3), IPSR (5), EPSR (6), or IEPSR (7). Reserved selectors return `None`.
pub fn read_psr_bits(current: u32, sysm: u8) -> Option<u32> {
    let mask = match sysm {
        0 | 2 => APSR,
        1 | 3 => APSR | 0x1ff,
        5 | 7 => 0x1ff,
        6 => 0,
        _ => return None,
    };
    Some(current & mask)
}
/// Apply the MSR APSR mask: bit 1 selects NZCVQ; bit 0 selects GE.
/// Preserve EPSR, IPSR and fields outside the mask.
/// GE writes are available only in DSP profiles; wider masks return `None`.
pub fn write_apsr_bits(current: u32, input: u32, mask: u8) -> Option<u32> {
    if mask > 3 {
        return None;
    }
    let fields = (if mask & 2 != 0 { 0xf8000000 } else { 0 })
        | if mask & 1 != 0 { APSR & 0xf0000 } else { 0 };
    Some((current & !fields) | (input & fields))
}
