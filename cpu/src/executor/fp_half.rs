//! Half storage formats. Conversion rounds once, including binary64 sources.
fn layout(size: usize) -> (u32, i32, u64) {
    if size == 32 {
        (23, 127, 255)
    } else {
        (52, 1023, 2047)
    }
}
pub(crate) fn from_half(bits: u16, size: usize, control: u32) -> (u64, u32) {
    let (fraction, bias, maximum) = layout(size);
    let sign = u64::from(bits >> 15) << (size - 1);
    let exponent = u32::from(bits >> 10 & 31);
    let payload = u32::from(bits & 1023);
    if exponent == 31 && control & (1 << 26) == 0 {
        if payload == 0 {
            return (sign | maximum << fraction, 0);
        }
        let flags = u32::from(payload & 512 == 0);
        if control & (1 << 25) != 0 {
            return (maximum << fraction | 1 << (fraction - 1), flags);
        }
        return (
            sign | maximum << fraction
                | u64::from(payload) << (fraction - 10)
                | 1 << (fraction - 1),
            flags,
        );
    }
    if exponent == 0 {
        if payload == 0 {
            return (sign, 0);
        }
        let lead = 31 - payload.leading_zeros();
        let target_exponent = (bias - 24 + lead as i32) as u64;
        return (
            sign | target_exponent << fraction
                | (u64::from(payload) << (fraction - lead) & ((1 << fraction) - 1)),
            0,
        );
    }
    (
        sign | ((exponent as i32 - 15 + bias) as u64) << fraction
            | u64::from(payload) << (fraction - 10),
        0,
    )
}
pub(crate) fn to_half(bits: u64, size: usize, control: u32) -> (u64, u32) {
    let (fraction, bias, maximum) = layout(size);
    let negative = bits >> (size - 1) != 0;
    let sign = u64::from(negative) << 15;
    let exponent = (bits >> fraction) & maximum;
    let payload = bits & ((1 << fraction) - 1);
    let alternative = control & (1 << 26) != 0;
    if exponent == maximum {
        if alternative {
            return (sign | if payload == 0 { 0x7fff } else { 0 }, 1);
        }
        if payload == 0 {
            return (sign | 0x7c00, 0);
        }
        let flags = u32::from(payload & (1 << (fraction - 1)) == 0);
        if control & (1 << 25) != 0 {
            return (0x7e00, flags);
        }
        return (sign | 0x7c00 | (payload >> (fraction - 10)) | 0x200, flags);
    }
    if exponent == 0 {
        if payload == 0 {
            return (sign, 0);
        }
        if control & (1 << 24) != 0 {
            return (sign, 128);
        }
    }
    let e = if exponent == 0 {
        1 - bias
    } else {
        exponent as i32 - bias
    };
    let significand = payload | if exponent == 0 { 0 } else { 1 << fraction };
    let tiny = e < -14;
    let shift = if tiny {
        fraction as i32 - 24 - e
    } else {
        fraction as i32 - 10
    } as u32;
    let truncated = if shift >= 64 { 0 } else { significand >> shift };
    let remainder = if shift >= 64 {
        significand
    } else {
        significand & ((1 << shift) - 1)
    };
    let halfway = if shift >= 64 {
        core::cmp::Ordering::Less
    } else {
        remainder.cmp(&(1 << (shift - 1)))
    };
    let mode = (control >> 22) & 3;
    let increment = remainder != 0
        && match mode {
            0 => halfway.is_gt() || (halfway.is_eq() && truncated & 1 != 0),
            1 => !negative,
            2 => negative,
            _ => false,
        };
    let rounded = truncated + u64::from(increment);
    let (target_exponent, mantissa) = if tiny {
        if rounded >= 1024 {
            (1, 0)
        } else {
            (0, rounded)
        }
    } else if rounded >= 2048 {
        (e + 16, 0)
    } else {
        (e + 15, rounded & 1023)
    };
    if target_exponent >= if alternative { 32 } else { 31 } {
        if alternative {
            return (sign | 0x7fff, 1);
        }
        let infinity = mode == 0 || (mode == 1 && !negative) || (mode == 2 && negative);
        return (sign | if infinity { 0x7c00 } else { 0x7bff }, 20);
    }
    let flags = if remainder == 0 {
        0
    } else if tiny {
        24
    } else {
        16
    };
    (sign | (target_exponent as u64) << 10 | mantissa, flags)
}
