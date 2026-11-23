//! Integer-backed IEEE arithmetic; ARM NaN selection and input FZ live in fp_generic.
use rustc_apfloat::{
    Float, FloatConvert, Round, Status, StatusAnd,
    ieee::{Double, Quad, Single},
};

fn operation<F: Float>(op: u8, a: F, b: F, addend: Option<F>, rounding: Round) -> StatusAnd<F> {
    if let Some(c) = addend {
        return a.mul_add_r(b, c, rounding);
    }
    match op {
        0 => a.add_r(b, rounding),
        1 => a.sub_r(b, rounding),
        2 => a.mul_r(b, rounding),
        3 => a.div_r(b, rounding),
        _ => unreachable!("binary floating-point operation"),
    }
}

fn inner<F: Float + FloatConvert<W>, W: Float>(
    op: u8,
    a: u64,
    b: u64,
    addend: Option<u64>,
    control: u32,
    minimum: u64,
) -> (u64, u32) {
    let rounding = match (control >> 22) & 3 {
        0 => Round::NearestTiesToEven,
        1 => Round::TowardPositive,
        2 => Round::TowardNegative,
        _ => Round::TowardZero,
    };
    let a = F::from_bits(u128::from(a));
    let b = F::from_bits(u128::from(b));
    let addend = addend.map(|c| F::from_bits(u128::from(c)));
    let result = operation(op, a, b, addend, rounding);
    let minimum = F::from_bits(u128::from(minimum));
    let mut flags = u32::from(result.status.bits());
    let mut tiny = result.value.is_denormal() || result.status.contains(Status::UNDERFLOW);
    let tiny_boundary = result.value.abs() == minimum && result.status.contains(Status::INEXACT);
    let overflow_boundary = result.value.abs() == F::largest()
        && result.status.contains(Status::INEXACT)
        && !result.status.contains(Status::OVERFLOW);
    if tiny_boundary || overflow_boundary {
        // LLVM tests tininess after rounding; ARM tests it before rounding.
        // W holds every input/product exponent. Rounding its magnitude toward
        // zero preserves which side of a representable power-of-two boundary
        // the exact result lies on, even for a tiny FMA perturbation.
        let mut loses_info = false;
        let wide_a: W = a.convert(&mut loses_info).value;
        let wide_b: W = b.convert(&mut loses_info).value;
        let wide_addend: Option<W> = addend.map(|c| c.convert(&mut loses_info).value);
        let wide_minimum: W = minimum.convert(&mut loses_info).value;
        let wide_result = operation(op, wide_a, wide_b, wide_addend, Round::TowardZero)
            .value
            .abs();
        if tiny_boundary {
            tiny = wide_result < wide_minimum;
        }
        // LLVM omits OFC when overflow rounds to a finite endpoint. ARM still
        // reports it if rounding with an unbounded exponent would overflow.
        if overflow_boundary && wide_result >= W::from_u128(1).value.scalbn(F::MAX_EXP + 1) {
            flags |= 4;
        }
    }
    if tiny && control & (1 << 24) != 0 {
        // FZ flushes even an exact subnormal, setting UFC without adding IXC.
        let zero = F::ZERO.copy_sign(result.value);
        return (zero.to_bits() as u64, (flags & !16) | 8);
    }
    if tiny && result.status.contains(Status::INEXACT) {
        flags |= 8;
    }
    (result.value.to_bits() as u64, flags)
}

pub(super) fn binary(op: u8, a: u64, b: u64, size: usize, control: u32) -> (u64, u32) {
    if size == 32 {
        inner::<Single, Double>(op, a, b, None, control, 0x0080_0000)
    } else {
        inner::<Double, Quad>(op, a, b, None, control, 0x0010_0000_0000_0000)
    }
}

pub(super) fn fused(a: u64, b: u64, c: u64, size: usize, control: u32) -> (u64, u32) {
    if size == 32 {
        inner::<Single, Double>(0, a, b, Some(c), control, 0x0080_0000)
    } else {
        inner::<Double, Quad>(0, a, b, Some(c), control, 0x0010_0000_0000_0000)
    }
}

pub(super) fn convert_precision(bits: u64, source_size: usize, control: u32) -> (u64, u32) {
    let (fraction, exponent, target_fraction, target_exponent, target_size) = if source_size == 32 {
        (23, 255, 52, 2047, 64)
    } else {
        (52, 2047, 23, 255, 32)
    };
    let negative = bits >> (source_size - 1) != 0;
    let sign = u64::from(negative) << (target_size - 1);
    let field = (bits >> fraction) & exponent;
    let payload = bits & ((1u64 << fraction) - 1);
    if field == exponent {
        if payload == 0 {
            return (sign | (target_exponent << target_fraction), 0);
        }
        let flags = u32::from(payload & (1 << (fraction - 1)) == 0);
        if control & (1 << 25) != 0 {
            return (
                (target_exponent << target_fraction) | (1 << (target_fraction - 1)),
                flags,
            );
        }
        let payload = if fraction < target_fraction {
            payload << (target_fraction - fraction)
        } else {
            payload >> (fraction - target_fraction)
        };
        return (
            sign | (target_exponent << target_fraction) | payload | (1 << (target_fraction - 1)),
            flags,
        );
    }
    if field == 0 {
        if payload == 0 {
            return (sign, 0);
        }
        if control & (1 << 24) != 0 {
            return (sign, 128);
        }
    }
    let rounding = match (control >> 22) & 3 {
        0 => Round::NearestTiesToEven,
        1 => Round::TowardPositive,
        2 => Round::TowardNegative,
        _ => Round::TowardZero,
    };
    let mut loses_info = false;
    if source_size == 32 {
        let result: StatusAnd<Double> =
            Single::from_bits(u128::from(bits)).convert_r(rounding, &mut loses_info);
        return (
            result.value.to_bits() as u64,
            u32::from(result.status.bits()),
        );
    }
    let source = Double::from_bits(u128::from(bits));
    let result: StatusAnd<Single> = source.convert_r(rounding, &mut loses_info);
    let mut flags = u32::from(result.status.bits());
    let tiny = source.abs() < Double::from_bits(0x3810_0000_0000_0000);
    if tiny && control & (1 << 24) != 0 {
        return (sign, (flags & !16) | 8);
    }
    if tiny && result.status.contains(Status::INEXACT) {
        flags |= 8;
    }
    if result.status.contains(Status::INEXACT)
        && source.abs() >= Double::from_bits(0x47f0_0000_0000_0000)
    {
        flags |= 4;
    }
    (result.value.to_bits() as u64, flags)
}

fn integer_sqrt(value: u128) -> u128 {
    // Restoring square root, two input bits per iteration; no host float or
    // convergence tolerance. The radicand uses at most 107 bits.
    let mut remainder = value;
    let mut root = 0u128;
    let mut bit = 1u128 << 126;
    while bit > remainder {
        bit >>= 2;
    }
    while bit != 0 {
        if remainder >= root + bit {
            remainder -= root + bit;
            root = (root >> 1) + bit;
        } else {
            root >>= 1;
        }
        bit >>= 2;
    }
    root
}

pub(super) fn square_root(bits: u64, size: usize, control: u32) -> (u64, u32) {
    let (fraction, bias, mask) = if size == 32 {
        (23, 127, 0xff)
    } else {
        (52, 1023, 0x7ff)
    };
    let encoded_exponent = ((bits >> fraction) & mask) as i32;
    let mut significand = bits & ((1u64 << fraction) - 1);
    let mut exponent = if encoded_exponent == 0 {
        1 - bias
    } else {
        encoded_exponent - bias
    };
    if encoded_exponent == 0 {
        let shift = fraction - (63 - significand.leading_zeros());
        significand <<= shift;
        exponent -= shift as i32;
    } else {
        significand |= 1u64 << fraction;
    }
    let radicand = u128::from(significand) << (fraction + (exponent & 1) as u32);
    let mut root = integer_sqrt(radicand);
    let remainder = radicand - root * root;
    let increment = match (control >> 22) & 3 {
        0 => remainder > root,
        1 => remainder != 0,
        _ => false,
    };
    root += u128::from(increment);
    let mut result_exponent = exponent.div_euclid(2) + bias;
    if root == 1u128 << (fraction + 1) {
        root >>= 1;
        result_exponent += 1;
    }
    let result = ((result_exponent as u64) << fraction) | (root as u64 & ((1u64 << fraction) - 1));
    (result, if remainder != 0 { 16 } else { 0 })
}

#[cfg(test)]
mod tests {
    use super::integer_sqrt;

    #[test]
    fn square_root_brackets_exact_squares_across_the_significand_range() {
        let mut seed = 0x37b4_c29e_5918_24fdu64;
        for i in 0..2_048 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let root = if i < 54 {
                1u128 << i
            } else {
                u128::from(seed & ((1 << 54) - 1)) + 1
            };
            let square = root * root;
            assert_eq!(integer_sqrt(square), root);
            assert_eq!(integer_sqrt(square - 1), root - 1);
            assert_eq!(integer_sqrt(square + 2 * root), root);
        }
        for value in [0, 1, 2, 3, 4, (1u128 << 107) - 1] {
            let root = integer_sqrt(value);
            assert!(root * root <= value && (root + 1) * (root + 1) > value);
        }
    }
}
