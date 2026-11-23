//! Pinned QEMU results for the saturation ARM register guest.
include!("saturation_probe_cases.rs");
include!("qadd_probe_cases.rs");
include!("parallel_probe_cases.rs");
pub const REGISTERS: [u32; 14] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 14];
pub fn expected_matrix() -> (u32, u64) {
    let mut count = 0;
    let mut hash = 0xcbf29ce484222325u64;
    for ([_, input, _, result, status], binary) in CASES
        .into_iter()
        .map(|row| (row, false))
        .chain(WORD_CASES.into_iter().map(|row| (row, true)))
        .chain(PARALLEL_CASES.into_iter().map(|row| (row, true)))
    {
        for destination in 0..14 {
            for source in 0..14 {
                let mut registers: [u32; 14] =
                    core::array::from_fn(|i| 0xdeadbeef ^ (REGISTERS[i] * 0x01010101));
                if binary {
                    registers[2] = input;
                }
                registers[source] = input;
                registers[destination] = result;
                for word in registers.into_iter().chain([status]) {
                    for byte in word.to_le_bytes() {
                        hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
                    }
                }
                count += 1;
            }
        }
    }
    (count, hash)
}
pub fn check(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() != 128 {
        return Err("ARM saturation capture length is not 128 bytes".into());
    }
    let words: [u32; 32] =
        core::array::from_fn(|i| u32::from_le_bytes(bytes[4 * i..4 * i + 4].try_into().unwrap()));
    if words[0] == 0xbad0f019 {
        return Err(format!(
            "ARM saturation failed at case {} opcode {:#x} register {} actual {:#x}",
            words[1], words[2], words[3], words[4]
        ));
    }
    if words[0] != 0xab00f019 || words[25] != 0 || words[26] != 0 || words[31] != 0 {
        return Err(format!("ARM saturation guest did not complete: {words:x?}"));
    }
    let (count, hash) = expected_matrix();
    if (
        words[27],
        u64::from(words[28]) | u64::from(words[29]) << 32,
        words[30],
    ) != (count, hash, 1)
    {
        return Err("ARM saturation register count/checksum differs from QEMU-pinned cases".into());
    }
    Ok(())
}
