//! QEMU-pinned expectations for the cross-backend fixed-point ARM guest.
include!("fixed_probe_cases.rs");

pub fn expected_matrix() -> (u32, u64) {
    let mut count = 0;
    let mut hash = 0xcbf29ce484222325u64;
    for [opcode, il, ih, _, lo, hi, status] in CASES {
        let double = opcode & (1 << 8) != 0;
        for register in 0..if double { 16 } else { 32 } {
            let start = if double { register * 2 } else { register };
            let mut words: [u32; 32] =
                core::array::from_fn(|i| 0xdeadbeef ^ (i as u32 * 0x01010101));
            words[start] = il;
            if double {
                words[start + 1] = ih;
            }
            words[start] = lo;
            if double {
                words[start + 1] = hi;
            }
            for word in words.into_iter().chain([status]) {
                for byte in word.to_le_bytes() {
                    hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
                }
            }
            count += 1;
        }
    }
    (count, hash)
}
pub fn check(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() != 128 {
        return Err("ARM fixed conversion capture length is not 128 bytes".into());
    }
    let words: [u32; 32] =
        core::array::from_fn(|i| u32::from_le_bytes(bytes[4 * i..4 * i + 4].try_into().unwrap()));
    if words[0] == 0xbad0f017 {
        return Err(format!(
            "ARM fixed register matrix failed at case {} opcode {:#x} register {} actual {:#x}",
            words[1], words[2], words[3], words[4]
        ));
    }
    if words[0] != 0xab00f017 || words[25] != 0 || words[26] != 0 || words[31] != 0 {
        return Err(format!(
            "ARM fixed conversion guest did not complete: {words:x?}"
        ));
    }
    let (count, hash) = expected_matrix();
    if (
        words[27],
        u64::from(words[28]) | u64::from(words[29]) << 32,
        words[30],
    ) != (count, hash, 1)
    {
        return Err(
            "ARM fixed register matrix count/checksum differs from QEMU-pinned expectations".into(),
        );
    }
    Ok(())
}
