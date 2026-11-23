//! QEMU-pinned expectations for the standalone long-multiply register guest.
include!("long_multiply_probe_cases.rs");
pub const REGISTERS: [u32; 14] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 14];
pub fn expected_matrix() -> (u32, u64) {
    let mut count = 0;
    let mut hash = 0xcbf29ce484222325u64;
    for [_, input, _, lo, hi, status] in LONG_CASES {
        for low in 0..14 {
            for high in 0..14 {
                if low == high {
                    continue;
                }
                for source in 0..14 {
                    let mut registers: [u32; 14] =
                        core::array::from_fn(|i| 0xdeadbeef ^ (REGISTERS[i] * 0x01010101));
                    registers[low] = input;
                    registers[high] = input;
                    registers[2] = input;
                    registers[source] = input;
                    registers[low] = lo;
                    registers[high] = hi;
                    for word in registers.into_iter().chain([status]) {
                        for byte in word.to_le_bytes() {
                            hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
                        }
                    }
                    count += 1;
                }
            }
        }
    }
    (count, hash)
}
pub fn check(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() != 128 {
        return Err("ARM long-multiply capture length is not 128 bytes".into());
    }
    let words: [u32; 32] =
        core::array::from_fn(|i| u32::from_le_bytes(bytes[4 * i..4 * i + 4].try_into().unwrap()));
    if words[0] == 0xbad0f023 {
        return Err(format!(
            "ARM long multiply failed at case {} opcode {:#x} register {} actual {:#x} expected {:#x}",
            words[1], words[2], words[3], words[4], words[5]
        ));
    }
    if words[0] != 0xab00f023 || words[25] != 0 || words[26] != 0 || words[31] != 0 {
        return Err(format!(
            "ARM long-multiply guest did not complete: {words:x?}"
        ));
    }
    let (count, hash) = expected_matrix();
    if (
        words[27],
        u64::from(words[28]) | (u64::from(words[29]) << 32),
        words[30],
    ) != (count, hash, 1)
    {
        return Err(
            "ARM long-multiply register count/checksum differs from QEMU-pinned cases".into(),
        );
    }
    Ok(())
}
