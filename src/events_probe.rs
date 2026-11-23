//! Independently specified sleep/event checkpoints from the executed ARM guest.
pub const STAGES: [u32; 9] = [1, 3, 3, 5, 5, 7, 9, 9, 12];
pub fn check(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() != 128 {
        return Err("ARM event capture length is not 128 bytes".into());
    }
    let words: [u32; 32] =
        core::array::from_fn(|i| u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap()));
    if words[0] != 0xab00f026
        || words[1] != 12
        || words[2] != 1
        || words[3] != 0xa80a0000
        || words[31] != 0
    {
        return Err(format!("ARM event guest did not complete: {words:x?}"));
    }
    if words[16..25] != STAGES {
        return Err(format!(
            "ARM sleep checkpoints differ: {:x?}",
            &words[16..25]
        ));
    }
    Ok(())
}
