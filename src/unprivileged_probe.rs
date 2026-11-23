//! ARM MPU checks with numerical expectations captured independently by QEMU.
include!("../cpu/fixtures/unprivileged_probe_cases.rs");
pub const WORDS: usize = 16 + 74 * 8;
pub fn check(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() != WORDS * 4 {
        return Err("ARM unprivileged capture length differs".into());
    }
    let words: Vec<_> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_le_bytes(*b))
        .collect();
    if words[0] != 0xab00f027 || words[1] != 74 || words[15] != 0 {
        return Err(format!(
            "ARM unprivileged guest did not complete: {:x?}",
            &words[..16]
        ));
    }
    let mut expected = Vec::new();
    for ap in [0, 1, 2, 3, 5, 6, 7] {
        for row in UNPRIVILEGED_CASES {
            if row[0] & 255 == 0 {
                let store = row[0] & (1 << 20) == 0;
                let allowed = ap == 3 || !store && matches!(ap, 2 | 6 | 7);
                expected.push((row, if allowed { 0 } else { 0x82 }, false));
            }
        }
    }
    for trap in [false, true] {
        for row in UNPRIVILEGED_CASES {
            if row[0] & 255 == 1 {
                let wide = matches!((row[0] >> 20) & 31, 2 | 3 | 4 | 5 | 19);
                expected.push((row, if trap && wide { 1 << 24 } else { 0 }, false));
            }
        }
    }
    for user in [false, true] {
        expected.push((
            [0xf8410e00, 5, u32::from(user) << 5, 0, 0xa80a0000],
            if user { 0 } else { 0x82 },
            true,
        ));
    }
    for (index, ([opcode, result, lo, hi, status], fault, stir)) in expected.into_iter().enumerate()
    {
        let actual = &words[16 + index * 8..16 + (index + 1) * 8];
        let before = if stir {
            [0, 0]
        } else {
            INITIAL_GUARDS[(opcode & 255) as usize]
        };
        let allowed = fault == 0;
        let wanted = [
            opcode,
            if allowed {
                result
            } else if stir {
                5
            } else {
                0x80008000
            },
            if allowed { lo } else { before[0] },
            if allowed { hi } else { before[1] },
            status,
            fault,
            if fault & 128 != 0 {
                if stir {
                    0xe000ef00
                } else {
                    0x24020004 + (opcode & 255)
                }
            } else {
                0
            },
            if stir { 0xe000ef00 } else { 0x24020004 },
        ];
        if actual != wanted {
            return Err(format!("ARM unprivileged case {index} opcode {opcode:#010x}: actual {actual:x?}, expected {wanted:x?}"));
        }
    }
    Ok(())
}
