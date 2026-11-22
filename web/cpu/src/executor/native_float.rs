//! Guarded native IEEE binary32/binary64 arithmetic with architectural flags.
//! Nondefault controls and unsupported special cases use the ARM reference path.
pub(super) fn ordinary(bits: u32) -> bool {
    let exponent = (bits >> 23) & 255;
    exponent != 255 && (exponent != 0 || bits & 0x7fffff == 0)
}
#[inline(always)]
pub(super) fn binary(op: u8, mut a: u32, mut b: u32, control: u32) -> Option<(u32, u32)> {
    if control & 0x00c09f00 != 0 || op > 3 {
        return None;
    }
    let mut flags = 0;
    if control & (1 << 24) != 0 {
        for bits in [&mut a, &mut b] {
            if *bits & 0x7f80_0000 == 0 && *bits & 0x007f_ffff != 0 {
                *bits &= 0x8000_0000;
                flags |= 128;
            }
        }
    }
    let ma = a & 0x7fff_ffff;
    let mb = b & 0x7fff_ffff;
    // ARM prioritizes signaling NaNs before quiet NaNs, preserving payloads
    // unless default-NaN mode is selected. No host NaN propagation is used.
    if ma > 0x7f80_0000 || mb > 0x7f80_0000 {
        let signal_a = ma > 0x7f80_0000 && a & 0x0040_0000 == 0;
        let signal_b = mb > 0x7f80_0000 && b & 0x0040_0000 == 0;
        let payload = if signal_a {
            a
        } else if signal_b {
            b
        } else if ma > 0x7f80_0000 {
            a
        } else {
            b
        };
        return Some((
            if control & (1 << 25) != 0 {
                0x7fc0_0000
            } else {
                payload | 0x0040_0000
            },
            flags | u32::from(signal_a || signal_b),
        ));
    }
    if ma == 0x7f80_0000 || mb == 0x7f80_0000 {
        let sign = (a ^ b) & 0x8000_0000;
        let invalid = match op {
            0 | 1 => ma == mb && ((a ^ b) >> 31 == u32::from(op == 0)),
            2 => ma == 0 || mb == 0,
            3 => ma == mb,
            _ => unreachable!(),
        };
        if invalid {
            return Some((0x7fc0_0000, flags | 1));
        }
        let result = match op {
            0 | 1 => {
                if ma == 0x7f80_0000 {
                    a
                } else {
                    b ^ (u32::from(op) << 31)
                }
            }
            2 => sign | 0x7f80_0000,
            3 => sign | if ma == 0x7f80_0000 { 0x7f80_0000 } else { 0 },
            _ => unreachable!(),
        };
        return Some((result, flags));
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
            return Some((bits, flags));
        }
        let exact_zero = match op {
            0 => a == b ^ (1 << 31) || x == 0. && y == 0.,
            1 => a == b || x == 0. && y == 0.,
            2 => x == 0. || y == 0.,
            3 => x == 0. && y != 0.,
            _ => false,
        };
        if bits & 0x7fff_ffff == 0 && exact_zero {
            return Some((bits, flags));
        }
    }
    let x = f64::from(f32::from_bits(a));
    let y = f64::from(f32::from_bits(b));
    if op == 3 && y == 0.0 {
        return Some((
            if x == 0. {
                0x7fc0_0000
            } else {
                (a ^ b) & 0x8000_0000 | 0x7f80_0000
            },
            flags | if x == 0. { 1 } else { 2 },
        ));
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
        return Some((bits, flags | 4 | 16));
    }
    if control & (1 << 24) != 0 && exact != 0.0 && exact.abs() < f64::from(f32::MIN_POSITIVE) {
        return Some((bits & 0x8000_0000, flags | 8));
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
        flags | if inexact { 16 } else { 0 } | if underflow { 8 } else { 0 },
    ))
}
pub(super) fn compare(
    mut a: u32,
    mut b: u32,
    control: u32,
    quiet_nan_exc: bool,
) -> Option<((bool, bool, bool, bool), u32)> {
    if control & 0x0000_9f00 != 0 {
        return None;
    }
    let mut flags = 0;
    if control & (1 << 24) != 0 {
        for bits in [&mut a, &mut b] {
            if *bits & 0x7f80_0000 == 0 && *bits & 0x007f_ffff != 0 {
                *bits &= 0x8000_0000;
                flags |= 128;
            }
        }
    }
    let nan_a = a & 0x7fff_ffff > 0x7f80_0000;
    let nan_b = b & 0x7fff_ffff > 0x7f80_0000;
    if nan_a || nan_b {
        flags |= u32::from(
            quiet_nan_exc || nan_a && a & 0x0040_0000 == 0 || nan_b && b & 0x0040_0000 == 0,
        );
        return Some(((false, false, true, true), flags));
    }
    let x = f32::from_bits(a);
    let y = f32::from_bits(b);
    Some((
        if x == y {
            (false, true, true, false)
        } else if x < y {
            (true, false, false, false)
        } else {
            (false, false, true, false)
        },
        flags,
    ))
}

/// A normal nonnegative binary32 square root cannot overflow or underflow.
/// Its rounded square is exact in binary64 (at most 48 significand bits), so
/// it also gives an exact test for ARM's cumulative inexact exception flag.
#[inline(always)]
pub(super) fn square_root(input: u32, control: u32) -> Option<(u32, u32)> {
    if control & 0x00c0_9f00 != 0 {
        return None;
    }
    let magnitude = input & 0x7fff_ffff;
    if magnitude > 0x7f80_0000 {
        return Some((
            if control & (1 << 25) != 0 {
                0x7fc0_0000
            } else {
                input | 0x0040_0000
            },
            u32::from(input & 0x0040_0000 == 0),
        ));
    }
    if magnitude != 0 && magnitude < 0x0080_0000 && control & (1 << 24) != 0 {
        return Some((input & 0x8000_0000, 128));
    }
    let value = f32::from_bits(input);
    if value < 0. {
        return Some((0x7fc0_0000, 1));
    }
    if magnitude == 0x7f80_0000 {
        return Some((input, 0));
    }
    let result = value.sqrt();
    let rounded = f64::from(result);
    let flags = if rounded * rounded != f64::from(value) {
        16
    } else {
        0
    };
    Some((result.to_bits(), flags))
}

#[inline(always)]
fn ordinary64(bits: u64) -> bool {
    let exponent = (bits >> 52) & 2047;
    exponent != 2047 && (exponent != 0 || bits & 0x000f_ffff_ffff_ffff == 0)
}
/// Decode a finite binary64 value into its integer significand and exponent.
fn parts64(bits: u64) -> (u64, i32) {
    let exponent = ((bits >> 52) & 2047) as i32;
    let fraction = bits & 0x000f_ffff_ffff_ffff;
    if exponent == 0 {
        (fraction, -1074)
    } else {
        (fraction | (1 << 52), exponent - 1023 - 52)
    }
}
/// Compare a 106-bit exact product to a binary64 value without rounding it.
fn product_equals64(a: u64, b: u64, c: u64) -> bool {
    let (ma, ea) = parts64(a);
    let (mb, eb) = parts64(b);
    let (mc, ec) = parts64(c);
    let product = u128::from(ma) * u128::from(mb);
    if product == 0 || mc == 0 {
        return product == 0 && mc == 0;
    }
    let trailing = product.trailing_zeros();
    let c_trailing = mc.trailing_zeros();
    product >> trailing == u128::from(mc >> c_trailing)
        && ea + eb + trailing as i32 == ec + c_trailing as i32
}
fn tiny64(op: u8, a: u64, b: u64) -> bool {
    let (ma, ea) = parts64(a);
    let (mb, eb) = parts64(b);
    if ma == 0 || mb == 0 {
        return false;
    }
    match op {
        2 => {
            let p = u128::from(ma) * u128::from(mb);
            let exponent = ea + eb + (127 - p.leading_zeros()) as i32;
            exponent < -1022
        }
        3 => {
            let la = 63 - ma.leading_zeros();
            let lb = 63 - mb.leading_zeros();
            let na = ma << (52 - la);
            let nb = mb << (52 - lb);
            let exponent = ea + la as i32 - eb - lb as i32 + (if na < nb { -1 } else { 0 });
            exponent < -1022
        }
        _ => false,
    }
}
/// IEEE round-to-nearest finite arithmetic. Exact integer products and TwoSum
/// preserve exception flags even where a rounded f64 check would lose a tail.
#[inline(always)]
pub(super) fn binary64(op: u8, a: u64, b: u64, control: u32) -> Option<(u64, u32)> {
    if control & 0x00c0_9f00 != 0 || (a >> 52) & 2047 == 2047 || (b >> 52) & 2047 == 2047 {
        return None;
    }
    if control & (1 << 24) != 0 && (!ordinary64(a) || !ordinary64(b)) {
        return None;
    }
    let x = f64::from_bits(a);
    let y = f64::from_bits(b);
    let value = match op {
        0 => x + y,
        1 => x - y,
        2 => x * y,
        3 => x / y,
        _ => return None,
    };
    let bits = value.to_bits();
    let magnitude = bits & 0x7fff_ffff_ffff_ffff;
    if magnitude > 0x7ff0_0000_0000_0000 {
        return None;
    }
    if magnitude == 0x7ff0_0000_0000_0000 {
        return Some((bits, if op == 3 && y == 0. { 2 } else { 4 | 16 }));
    }
    if magnitude > 0x0010_0000_0000_0000 && control & 16 != 0 {
        return Some((bits, 0));
    }
    let inexact = match op {
        0 | 1 => {
            // Knuth TwoSum recovers the exact rounding residual.
            let right = if op == 1 { -y } else { y };
            let virtual_right = value - x;
            (x - (value - virtual_right)) + (right - virtual_right) != 0.
        }
        2 => !product_equals64(a, b, bits),
        3 => !product_equals64(bits, b, a),
        _ => unreachable!(),
    };
    let tiny =
        magnitude < 0x0010_0000_0000_0000 || magnitude == 0x0010_0000_0000_0000 && tiny64(op, a, b);
    if control & (1 << 24) != 0 && tiny && (inexact || magnitude != 0) {
        return None;
    }
    Some((
        bits,
        if inexact { 16 } else { 0 } | if tiny && inexact { 8 } else { 0 },
    ))
}
pub(super) fn compare64(a: u64, b: u64, control: u32) -> Option<(bool, bool, bool, bool)> {
    if a & 0x7fff_ffff_ffff_ffff > 0x7ff0_0000_0000_0000
        || b & 0x7fff_ffff_ffff_ffff > 0x7ff0_0000_0000_0000
    {
        return None;
    }
    if control & (1 << 24) != 0
        && ((a >> 52) & 2047 == 0 && !ordinary64(a) || (b >> 52) & 2047 == 0 && !ordinary64(b))
    {
        return None;
    }
    let x = f64::from_bits(a);
    let y = f64::from_bits(b);
    Some(if x == y {
        (false, true, true, false)
    } else if x < y {
        (true, false, false, false)
    } else {
        (false, false, true, false)
    })
}
