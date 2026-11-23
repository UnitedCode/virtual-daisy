//! A literal ITCM sequence checks progress within one translated block.
pub fn check(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() != 128 {
        return Err("ARM instruction clock capture length is not 128 bytes".into());
    }
    let words: Vec<_> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&b| u32::from_le_bytes(b))
        .collect();
    if words[0] != 0xab00_c10c || words[20] != 0 || words[21] != 0 || words[31] != 0 {
        return Err("ARM instruction clock guest did not complete without faults".into());
    }
    let nops = words[3].wrapping_sub(words[1]);
    // Renode's integer-nanosecond conversion at 64 MHz groups eight cycles.
    // This proves instruction progress, not physical Cortex-M7 pipeline timing.
    if nops.abs_diff(128) > 8 {
        return Err(format!(
            "ARM instruction clock: 128 NOPs advanced {nops} cycles"
        ));
    }
    Ok(())
}
