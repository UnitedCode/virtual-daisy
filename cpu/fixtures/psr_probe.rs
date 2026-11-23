//! QEMU-pinned PSR mask and privilege expectations for the ARM guest.
include!("psr_probe_cases.rs");
pub fn expected() -> (u32, u64) {
    let mut hash = 0xcbf29ce484222325u64;
    for [_, _, _, _, apsr, xpsr, epsr] in CASES {
        for word in [apsr, xpsr, epsr, apsr, 0] {
            for byte in word.to_le_bytes() {
                hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
            }
        }
    }
    (CASES.len() as u32, hash)
}
pub fn check(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() != 128 {
        return Err("ARM PSR capture length is not 128 bytes".into());
    }
    let words: [u32; 32] =
        core::array::from_fn(|i| u32::from_le_bytes(bytes[4 * i..4 * i + 4].try_into().unwrap()));
    if words[0] == 0xbad0f119 {
        return Err(format!(
            "ARM PSR case {} failed: actual {:x?}",
            words[1],
            &words[2..7]
        ));
    }
    if words[0] != 0xab00f119 || words[25] != 0 || words[26] != 0 || words[31] != 0 {
        return Err(format!("ARM PSR guest did not complete: {words:x?}"));
    }
    let (count, hash) = expected();
    if (
        words[27],
        u64::from(words[28]) | u64::from(words[29]) << 32,
        words[30],
    ) != (count, hash, 1)
    {
        return Err("ARM PSR result count/checksum differs from the independent fixture".into());
    }
    Ok(())
}
