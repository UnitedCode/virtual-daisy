//! Small cross-backend guest; expected values are pinned to the ARM oracle.
pub const CASES: [(u32, u64, u32, [u32; 3]); 8] = [
    (0, 0xa5a53c00, 0xa0000000, [0x3f800000, 0, 0xa0000000]),
    (1, 0xc0005a5a, 0xa0000000, [0xc0000000, 0, 0xa0000000]),
    (2, 0x3f801001, 0xa0000000, [0xa5a53c01, 0, 0xa0000010]),
    (3, 0x7f800000, 0xa0000000, [0x7c005a5a, 0, 0xa0000000]),
    (4, 0xa5a57fff, 0xa4000000, [0, 0x40fffc00, 0xa4000000]),
    (5, 0x7d015a5a, 0xa2000000, [0, 0x7ff80000, 0xa2000001]),
    (
        6,
        0x3ff0020000000001,
        0xa0000000,
        [0xa5a53c01, 0, 0xa0000010],
    ),
    (
        7,
        0x7ff0000000000000,
        0xa4000000,
        [0x7fff5a5a, 0, 0xa4000001],
    ),
];

pub fn expected_register_matrix() -> (u32, u64) {
    let mut hash = 0xcbf29ce484222325u64;
    let mut count = 0;
    for (op, input, _, result) in CASES {
        let double = op >= 4;
        let to_half = op % 4 >= 2;
        for half in 0..32 {
            for full in 0..if double { 16 } else { 32 } {
                let mut words: [u32; 32] =
                    core::array::from_fn(|i| 0xdeadbeef ^ (i as u32 * 0x01010101));
                if to_half {
                    words[half] = 0xa5a55a5a;
                    if double {
                        words[2 * full] = input as u32;
                        words[2 * full + 1] = (input >> 32) as u32;
                    } else {
                        words[full] = input as u32;
                    }
                    let mask = 0xffffu32 << (16 * (op & 1));
                    words[half] = (words[half] & !mask) | (result[0] & mask);
                } else {
                    words[half] = input as u32;
                    if double {
                        words[2 * full] = result[0];
                        words[2 * full + 1] = result[1];
                    } else {
                        words[full] = result[0];
                    }
                }
                for word in words.into_iter().chain([result[2]]) {
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
        return Err("ARM half conversion capture length is not 128 bytes".into());
    }
    let words: Vec<_> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&word| u32::from_le_bytes(word))
        .collect();
    if words[0] == 0xbad0f016 {
        return Err(format!(
            "ARM half register matrix failed at case {} opcode {:#x} register {} actual {:#x}",
            words[27], words[28], words[29], words[30]
        ));
    }
    if words[0] != 0xab00f016 || words[25] != 0 || words[26] != 0 || words[31] != 0 {
        return Err("ARM half conversion guest did not complete without faults".into());
    }
    for (i, (_, _, _, expected)) in CASES.iter().enumerate() {
        if words[1 + i * 3..4 + i * 3] != *expected {
            return Err(format!(
                "ARM half conversion case {i}: expected {expected:x?}, got {:x?}",
                &words[1 + i * 3..4 + i * 3]
            ));
        }
    }
    let (count, hash) = expected_register_matrix();
    if (
        words[27],
        u64::from(words[28]) | u64::from(words[29]) << 32,
        words[30],
    ) != (count, hash, 1)
    {
        return Err(
            "ARM half register matrix count/checksum did not match its independently pinned cases"
                .into(),
        );
    }
    Ok(())
}

pub fn check_legacy_double(bytes: &[u8], site: &[u8]) -> Result<(), String> {
    if bytes.len() != 128 || site.len() != 4 {
        return Err("ARM half legacy control capture is incomplete".into());
    }
    let words: Vec<_> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&word| u32::from_le_bytes(word))
        .collect();
    for (i, (_, _, _, expected)) in CASES[..4].iter().enumerate() {
        if words[1 + 3 * i..4 + 3 * i] != *expected {
            return Err("ARM half legacy control did not complete its scalar conversions".into());
        }
    }
    if words[0] != 0
        || words[25] != 0x10000
        || words[26] != 0x40000000
        || words[31] != 3
        || u32::from_le_bytes(site.try_into().unwrap()) != 0xeeb20b41
    {
        return Err("ARM half legacy control did not trap at the first double conversion".into());
    }
    Ok(())
}
