//! Independent clock-ratio gate shared by the two execution backends.
pub fn check(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() != 128 {
        return Err("ARM SysTick capture length is not 128 bytes".into());
    }
    let words: Vec<_> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&b| u32::from_le_bytes(b))
        .collect();
    if words[0] != 0xab || words[31] != 0 {
        return Err("ARM SysTick guest did not complete normally".into());
    }
    for (phase, divider, control) in [(0, 8, 1), (1, 1, 5)] {
        let at = 1 + phase * 5;
        if words[at] & 7 != control {
            return Err(format!(
                "ARM SysTick phase {phase} CLKSOURCE: expected {control}, got {}",
                words[at] & 7
            ));
        }
        let ticks = words[at + 1].wrapping_sub(words[at + 2]) & 0xffffff;
        let cycles = words[at + 4].wrapping_sub(words[at + 3]);
        if !(8192..=9000).contains(&cycles)
            || (u64::from(ticks) * divider).abs_diff(u64::from(cycles)) > 128
        {
            return Err(format!("ARM SysTick phase {phase}: {ticks} ticks for {cycles} core cycles, divider {divider}"));
        }
    }
    Ok(())
}
