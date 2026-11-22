//! Fast IEEE binary32 operations for ordinary finite operands, round-to-nearest,
//! with traps disabled. Exceptional/denormal cases use the reference ARM path.
pub(super) fn ordinary(bits: u32) -> bool {
    let exponent = (bits >> 23) & 255;
    exponent != 255 && (exponent != 0 || bits & 0x7fffff == 0)
}
#[inline(always)]
pub(super) fn binary(op: u8, a: u32, b: u32, control: u32) -> Option<(u32, u32)> {
    if control & 0x00c09f00 != 0 || (a >> 23) & 255 == 255 || (b >> 23) & 255 == 255 {
        return None;
    }
    if control & (1 << 24) != 0 && (!ordinary(a) || !ordinary(b)) {
        return None;
    }
    // IXC is cumulative. Once it is set, normal results cannot add any new
    // exception flags, so there is no need to reconstruct the exact f64 result
    // merely to set IXC again. Exceptional/underflow cases retain the full path.
    if control & 16 != 0 && ordinary(a) && ordinary(b) {
        let x = f32::from_bits(a);
        let y = f32::from_bits(b);
        let value = match op {
            0 => x + y,
            1 => x - y,
            2 => x * y,
            3 => x / y,
            _ => return None,
        };
        let bits = value.to_bits();
        let exponent = (bits >> 23) & 255;
        if exponent != 0 && exponent != 255 && bits & 0x7fff_ffff > 0x0080_0000 {
            return Some((bits, 0));
        }
        let exact_zero = match op {
            0 => a == b ^ (1 << 31) || x == 0. && y == 0.,
            1 => a == b || x == 0. && y == 0.,
            2 => x == 0. || y == 0.,
            3 => x == 0. && y != 0.,
            _ => false,
        };
        if bits & 0x7fff_ffff == 0 && exact_zero {
            return Some((bits, 0));
        }
    }
    let x = f64::from(f32::from_bits(a));
    let y = f64::from(f32::from_bits(b));
    if op == 3 && y == 0.0 {
        return None;
    }
    let exact = match op {
        0 => x + y,
        1 => x - y,
        2 => x * y,
        3 => x / y,
        _ => return None,
    };
    let rounded = exact as f32;
    let bits = rounded.to_bits();
    if (bits >> 23) & 255 == 255 {
        return None;
    }
    if control & (1 << 24) != 0
        && (!ordinary(bits) || (exact != 0.0 && exact.abs() < f64::from(f32::MIN_POSITIVE)))
    {
        return None;
    }
    let inexact = if op == 3 {
        f64::from(rounded) * y != x
    } else {
        exact != f64::from(rounded)
            || (op < 2 && x != 0.0 && y != 0.0 && ((a >> 23) & 255).abs_diff((b >> 23) & 255) > 53)
    };
    let underflow = inexact && exact != 0.0 && exact.abs() < f64::from(f32::MIN_POSITIVE);
    Some((
        bits,
        if inexact { 16 } else { 0 } | if underflow { 8 } else { 0 },
    ))
}
pub(super) fn compare(a: u32, b: u32) -> Option<(bool, bool, bool, bool)> {
    if !ordinary(a) || !ordinary(b) {
        return None;
    }
    let x = f32::from_bits(a);
    let y = f32::from_bits(b);
    Some(if x == y {
        (false, true, true, false)
    } else if x < y {
        (true, false, false, false)
    } else {
        (false, false, true, false)
    })
}
