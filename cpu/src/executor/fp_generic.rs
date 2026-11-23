// Modified for Virtual Daisy's browser backend; see UPSTREAM.md.
use std::ops::{Add, AddAssign, Div, Sub};

use crate::{
    Processor,
    bus::Bus,
    core::{
        bits::Bits,
        fault::Fault,
        fpregister::{FPSCRRounding, Fpscr, IntegralRounding},
        register::{ExtensionRegOperations, SingleReg},
    },
    peripheral::{
        mpu::{AccType, Mpu},
        scb::{
            FPCCR_ASPEN, FPCCR_LSPACT, FPCCR_USER, FPSCR_STATUS_CONTROL_END,
            FPSCR_STATUS_CONTROL_START,
        },
    },
};

use num_bigfloat::BigFloat;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FPType {
    Nonzero,
    Zero,
    Infinity,
    QNaN,
    SNaN,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FPExc {
    InvalidOp,
    DivideByZero,
    Overflow,
    Underflow,
    Inexact,
    InputDenorm,
}

// i128 is used to represent the "integer" pseudocode type in the Architecture Reference Manual
fn round_down(value: BigFloat) -> i128 {
    // largest integer n so that n <= x
    let floored = value.floor();

    if floored.is_nan() {
        return 0;
    }

    floored.to_i128().unwrap_or(if value < BigFloat::default() {
        i128::MIN + 1
    } else {
        i128::MAX
    })
}

fn ieee_layout(bits: usize) -> (u32, i32, u64) {
    if bits == 32 {
        (23, 127, 0xff)
    } else {
        (52, 1023, 0x7ff)
    }
}

fn integer_operand(cpu: &mut Processor, bits: u64, size: usize, control: u32) -> (FPType, bool) {
    let (fraction, _, exponent_mask) = ieee_layout(size);
    let sign = bits >> (size - 1) != 0;
    let exponent = (bits >> fraction) & exponent_mask;
    let mantissa = bits & ((1u64 << fraction) - 1);
    let kind = if exponent == exponent_mask {
        if mantissa == 0 {
            FPType::Infinity
        } else if mantissa & (1u64 << (fraction - 1)) == 0 {
            FPType::SNaN
        } else {
            FPType::QNaN
        }
    } else if exponent == 0 && mantissa == 0 {
        FPType::Zero
    } else if exponent == 0 && control.get_fz() {
        cpu.fp_process_exception(FPExc::InputDenorm, control);
        FPType::Zero
    } else {
        FPType::Nonzero
    };
    (kind, sign)
}

// Work on the IEEE significand directly: tiny binary64 values must not vanish
// during a generic-real subtraction. The targets are 16/32/64-bit integers;
// a u128::MAX sentinel is already beyond every representable target bound.
fn rounded_magnitude(
    bits: u64,
    size: usize,
    scale: usize,
    rounding: IntegralRounding,
) -> (u128, bool) {
    let (fraction, bias, exponent_mask) = ieee_layout(size);
    let exponent = ((bits >> fraction) & exponent_mask) as i32;
    let significand = u128::from(bits & ((1u64 << fraction) - 1))
        | if exponent == 0 { 0 } else { 1u128 << fraction };
    let power = (if exponent == 0 {
        1 - bias
    } else {
        exponent - bias
    } - fraction as i32)
        .saturating_add(i32::try_from(scale).unwrap_or(i32::MAX));
    if power >= 0 {
        let shift = power as u32;
        if shift >= 128 || significand > u128::MAX >> shift {
            return (u128::MAX, false);
        }
        return (significand << shift, false);
    }
    let shift = (-power) as u32;
    let (integer, remainder, half) = if shift >= 128 {
        (0, significand, core::cmp::Ordering::Less)
    } else {
        let remainder = significand & ((1u128 << shift) - 1);
        (
            significand >> shift,
            remainder,
            remainder.cmp(&(1u128 << (shift - 1))),
        )
    };
    let fractional = remainder != 0;
    let sign = bits >> (size - 1) != 0;
    let increment = match rounding {
        IntegralRounding::NearestEven => half.is_gt() || (half.is_eq() && integer & 1 != 0),
        IntegralRounding::NearestAway => !half.is_lt(),
        IntegralRounding::PlusInfinity => fractional && !sign,
        IntegralRounding::MinusInfinity => fractional && sign,
        IntegralRounding::TowardZero => false,
    };
    (integer + u128::from(increment), fractional)
}

fn saturate_magnitude(magnitude: u128, sign: bool, width: usize, unsigned: bool) -> (u64, bool) {
    debug_assert!((1..=64).contains(&width));
    let mask = u64::MAX >> (64 - width);
    if unsigned && sign {
        return (0, magnitude != 0);
    }
    let bound = if unsigned {
        u128::from(mask)
    } else {
        (1u128 << (width - 1)) - u128::from(!sign)
    };
    let clamped = magnitude.min(bound) as u64;
    let result = if sign {
        0u64.wrapping_sub(clamped) & mask
    } else {
        clamped
    };
    (result, magnitude > bound)
}

pub(super) fn float_to_fixed_bits(
    cpu: &mut Processor,
    bits: u64,
    size: usize,
    width: usize,
    fraction_bits: usize,
    unsigned: bool,
    rounding: IntegralRounding,
    control: u32,
) -> u64 {
    let (kind, sign) = integer_operand(cpu, bits, size, control);
    if kind == FPType::SNaN || kind == FPType::QNaN {
        cpu.fp_process_exception(FPExc::InvalidOp, control);
        return 0;
    }
    if kind == FPType::Infinity {
        cpu.fp_process_exception(FPExc::InvalidOp, control);
        return saturate_magnitude(u128::MAX, sign, width, unsigned).0;
    }
    if kind == FPType::Zero {
        return 0;
    }
    let (magnitude, fractional) = rounded_magnitude(bits, size, fraction_bits, rounding);
    let (result, overflow) = saturate_magnitude(magnitude, sign, width, unsigned);
    if overflow {
        cpu.fp_process_exception(FPExc::InvalidOp, control);
    } else if fractional {
        cpu.fp_process_exception(FPExc::Inexact, control);
    }
    result
}

/// Trait for floating point operations
/// This trait is generic over the underlying integer type (u32 or u64)
/// and provides methods for floating point operations.
pub trait FloatOps {
    // underlying integer type (u32 or u64)
    type Bits: PartialOrd
        + AddAssign
        + Div<Output = Self::Bits>
        + Sub<Output = Self::Bits>
        + Into<u64>
        + Copy;

    // signed version of the underlying integer type
    type SignedBits: Sized
        + PartialOrd
        + Default
        + Add<Output = Self::SignedBits>
        + AddAssign
        + Sub<Output = Self::SignedBits>
        + Ord
        + Copy
        + Into<BigFloat>;

    /// Number of bits in the underlying integer type
    fn native_binary(
        _op: u8,
        _a: Self::Bits,
        _b: Self::Bits,
        _control: u32,
    ) -> Option<(Self::Bits, u32)> {
        None
    }
    fn native_compare(
        _a: Self::Bits,
        _b: Self::Bits,
        _control: u32,
        _quiet_nan_exc: bool,
    ) -> Option<((bool, bool, bool, bool), u32)> {
        None
    }
    fn as_u32(_bits: Self::Bits) -> Option<u32> {
        None
    }
    fn from_u32(_bits: u32) -> Option<Self::Bits> {
        None
    }

    fn n() -> usize;

    fn is_zero(value: Self::Bits) -> bool;

    /// Returns the default NaN value
    fn fp_default_nan() -> Self::Bits;
    fn fp_infinity(sign: bool) -> Self::Bits;
    fn fp_zero(sign: bool) -> Self::Bits;
    fn zero() -> Self::Bits;
    fn set_bit(value: Self::Bits, bit: usize, set: bool) -> Self::Bits;

    fn powf2(e: BigFloat) -> BigFloat;

    fn unsigned_pow2_f(e: usize) -> BigFloat;

    fn integer_to_bits(value: i128) -> Self::Bits;
    fn signed_pow2(e: usize) -> Self::SignedBits;
    fn signed_value(value: i32) -> Self::SignedBits;

    fn fp_abs(value: Self::Bits) -> Self::Bits;
    fn fp_max_normal(sign: bool) -> Self::Bits;
    fn from_integer(value: u64) -> Self::Bits;

    fn unsigned_to_signed(value: Self::Bits) -> Self::SignedBits;
    fn bits_to_bigfloat(value: Self::Bits) -> BigFloat;
    fn signedbits_to_bigfloat(value: Self::SignedBits) -> BigFloat;

    fn fp_unpack(fpval: Self::Bits, fpscr_val: u32) -> (FPType, bool, BigFloat, Option<FPExc>);

    fn normalize(mantissa: BigFloat) -> (BigFloat, Self::SignedBits);
    fn concate_bits(
        sign: bool,
        biased_exp: Self::SignedBits,
        int_mantissa: Self::Bits,
    ) -> Self::Bits;
}

pub trait FloatingPointChecks {
    fn check_vfp_enabled(&mut self) -> Result<(), Fault>;
    fn execute_fp_check(&mut self) -> Result<(), Fault>;
    fn fp_process_exception(&mut self, exc: FPExc, fpscr_val: u32);
}

impl Processor {
    fn fp_binary_ieee<T: FloatOps>(
        &mut self,
        operation: u8,
        op1: T::Bits,
        op2: T::Bits,
        control: u32,
    ) -> T::Bits {
        let (type1, sign1) = integer_operand(self, op1.into(), T::n(), control);
        let (type2, sign2) = integer_operand(self, op2.into(), T::n(), control);
        let (done, result) = self.fp_process_nans::<T>(type1, type2, op1, op2, control);
        if done {
            return result;
        }
        let a = if type1 == FPType::Zero {
            T::fp_zero(sign1)
        } else {
            op1
        };
        let b = if type2 == FPType::Zero {
            T::fp_zero(sign2)
        } else {
            op2
        };
        let (bits, flags) = super::fp_ieee::binary(operation, a.into(), b.into(), T::n(), control);
        self.fpscr |= flags;
        if flags & 1 != 0 {
            T::fp_default_nan()
        } else {
            T::from_integer(bits)
        }
    }

    fn preserve_fp_state(&mut self) -> Result<(), Fault> {
        let acctype = if !self.fpccr.get_bit(FPCCR_USER) {
            AccType::Normal
        } else {
            AccType::UnPriv
        };

        for i in 0..16 {
            let memaddrdesc =
                self.validate_address(self.fpcar.wrapping_add(i * 4), acctype, true)?;
            let reg = SingleReg::from(i as u8);
            let value = self.get_sr(reg);
            self.write32(memaddrdesc, value)?;
        }
        let memaddrdesc = self.validate_address(self.fpcar.wrapping_add(0x40), acctype, true)?;
        self.write32(memaddrdesc, self.fpscr)?;
        self.fpccr.set_bit(FPCCR_LSPACT, false);
        Ok(())
    }
}

pub trait FloatingPointPublicOperations {
    fn fp_minmax_num<T: FloatOps>(&mut self, op1: T::Bits, op2: T::Bits, maximum: bool) -> T::Bits;
    fn fp_add<T: FloatOps>(
        &mut self,
        op1: T::Bits,
        op2: T::Bits,
        fpscr_controlled: bool,
    ) -> T::Bits;

    fn fp_sub<T: FloatOps>(
        &mut self,
        op1: T::Bits,
        op2: T::Bits,
        fpscr_controlled: bool,
    ) -> T::Bits;

    fn fp_mul_add<T: FloatOps>(
        &mut self,
        addend: T::Bits,
        op1: T::Bits,
        op2: T::Bits,
        fpscr_controlled: bool,
    ) -> T::Bits;

    fn fp_mul<T: FloatOps>(
        &mut self,
        op1: T::Bits,
        op2: T::Bits,
        fpscr_controlled: bool,
    ) -> T::Bits;

    fn fp_div<T: FloatOps>(
        &mut self,
        op1: T::Bits,
        op2: T::Bits,
        fpscr_controlled: bool,
    ) -> T::Bits;

    fn fp_compare<T: FloatOps>(
        &mut self,
        op1: T::Bits,
        op2: T::Bits,
        quiet_nan_exc: bool,
        fpscr_controlled: bool,
    ) -> (bool, bool, bool, bool);

    fn fp_abs<T: FloatOps>(&mut self, op: T::Bits) -> T::Bits;

    fn fp_sqrt<T: FloatOps>(&mut self, op: T::Bits, fpscr_controlled: bool) -> T::Bits;

    fn fp_round_int<T: FloatOps>(
        &mut self,
        op: T::Bits,
        zero_rounding: bool,
        exact: bool,
        fpscr_controlled: bool,
    ) -> T::Bits;

    fn fp_round_int_with_rounding<T: FloatOps>(
        &mut self,
        op: T::Bits,
        rounding: Option<IntegralRounding>,
        exact: bool,
        fpscr_controlled: bool,
    ) -> T::Bits;

    fn fp_to_fixed<N: FloatOps, M: FloatOps>(
        &mut self,
        op: N::Bits,
        fraction_bits: usize,
        unsigned: bool,
        round_towards_zero: bool,
        fpscr_controlled: bool,
    ) -> M::Bits;

    fn fp_to_fixed_with_rounding<N: FloatOps, M: FloatOps>(
        &mut self,
        op: N::Bits,
        fraction_bits: usize,
        unsigned: bool,
        rounding: Option<IntegralRounding>,
        fpscr_controlled: bool,
    ) -> M::Bits;

    fn fixed_to_fp<N: FloatOps, M: FloatOps>(
        &mut self,
        op: M::Bits,
        fraction_bits: usize,
        unsigned: bool,
        round_to_nearest: bool,
        fpscr_controlled: bool,
    ) -> N::Bits;
}

trait FloatingPointHiddenOperations {
    fn fp_process_nan<T: FloatOps>(
        &mut self,
        type1: FPType,
        op: T::Bits,
        fpscr_val: u32,
    ) -> T::Bits;

    fn fp_process_nans<T: FloatOps>(
        &mut self,
        type1: FPType,
        type2: FPType,
        op1: T::Bits,
        op2: T::Bits,
        fpscr_val: u32,
    ) -> (bool, T::Bits);

    fn fp_process_nans3<T: FloatOps>(
        &mut self,
        typea: FPType,
        type1: FPType,
        type2: FPType,
        addend: T::Bits,
        op1: T::Bits,
        op2: T::Bits,
        fpscr_val: u32,
    ) -> (bool, T::Bits);

    fn fp_unpack<T: FloatOps>(
        &mut self,
        fpval: T::Bits,
        fpscr_val: u32,
    ) -> (FPType, bool, BigFloat);

    fn fp_round<T: FloatOps>(&mut self, value: BigFloat, fpscr_val: u32) -> T::Bits;
}

impl FloatOps for u32 {
    fn native_binary(op: u8, a: u32, b: u32, control: u32) -> Option<(u32, u32)> {
        super::native_float::binary(op, a, b, control)
    }
    fn native_compare(
        a: u32,
        b: u32,
        control: u32,
        quiet_nan_exc: bool,
    ) -> Option<((bool, bool, bool, bool), u32)> {
        super::native_float::compare(a, b, control, quiet_nan_exc)
    }
    fn as_u32(bits: u32) -> Option<u32> {
        Some(bits)
    }
    fn from_u32(bits: u32) -> Option<u32> {
        Some(bits)
    }
    type Bits = u32;
    type SignedBits = i32;

    fn n() -> usize {
        32
    }

    fn zero() -> Self::Bits {
        0
    }

    fn set_bit(value: Self::Bits, bit: usize, set: bool) -> Self::Bits {
        let mut val = value;
        val.set_bit(bit, set);
        val
    }

    fn powf2(e: BigFloat) -> BigFloat {
        BigFloat::from(2).pow(&e)
    }

    fn integer_to_bits(value: i128) -> Self::Bits {
        value as Self::Bits
    }

    fn unsigned_pow2_f(e: usize) -> BigFloat {
        BigFloat::from(2).pow(&((e as u32).into()))
    }

    fn signed_pow2(e: usize) -> Self::SignedBits {
        2i32.pow(e as u32)
    }

    fn signed_value(value: i32) -> Self::SignedBits {
        value
    }

    fn unsigned_to_signed(value: Self::Bits) -> Self::SignedBits {
        value as Self::SignedBits
    }

    fn bits_to_bigfloat(value: Self::Bits) -> BigFloat {
        BigFloat::from(value)
    }

    fn signedbits_to_bigfloat(value: Self::SignedBits) -> BigFloat {
        BigFloat::from(value)
    }

    fn from_integer(value: u64) -> Self::Bits {
        value as Self::Bits
    }

    fn fp_infinity(sign: bool) -> Self::Bits {
        if sign { 0xFF80_0000 } else { 0x7F80_0000 }
    }

    fn fp_zero(sign: bool) -> Self::Bits {
        if sign { 0x8000_0000 } else { 0x0000_0000 }
    }

    fn fp_default_nan() -> Self::Bits {
        0x7FC0_0000
    }

    fn fp_abs(value: Self::Bits) -> Self::Bits {
        value & 0x7FFF_FFFF
    }

    fn is_zero(value: Self::Bits) -> bool {
        value == 0
    }

    fn fp_unpack(fpval: Self::Bits, fpscr_val: u32) -> (FPType, bool, BigFloat, Option<FPExc>) {
        let sign = fpval.get_bit(31);
        let exp32 = fpval.get_bits(23..31);
        let frac32 = fpval.get_bits(0..23);
        let mut exception = None;

        let (ret_type, mut value) = if Self::is_zero(exp32) {
            if Self::is_zero(frac32) || fpscr_val.get_bit(24) {
                if !Self::is_zero(frac32) {
                    // Denormalized input flushed to zero
                    exception = Some(FPExc::InputDenorm);
                }
                (FPType::Zero, BigFloat::default())
            } else {
                (
                    FPType::Nonzero,
                    BigFloat::from(2.0).pow(&BigFloat::from(-126.0))
                        * (BigFloat::from(frac32)
                            * BigFloat::from(2.0).pow(&BigFloat::from(-23.0))),
                )
            }
        } else if is_ones_32(exp32, 8) {
            if Self::is_zero(frac32) {
                (
                    FPType::Infinity,
                    BigFloat::from(2.0).pow(&BigFloat::from(1_000_000.0)),
                )
            } else {
                (
                    if frac32.get_bit(22) {
                        FPType::QNaN
                    } else {
                        FPType::SNaN
                    },
                    BigFloat::default(),
                )
            }
        } else {
            (
                FPType::Nonzero,
                BigFloat::from(2.0).pow(&(BigFloat::from(exp32) - BigFloat::from(127.0)))
                    * (BigFloat::from(1.0)
                        + BigFloat::from(frac32) * BigFloat::from(2.0).pow(&BigFloat::from(-23.0))),
            )
        };
        if sign {
            value = -value;
        }
        (ret_type, sign, value, exception)
    }

    fn fp_max_normal(sign: bool) -> Self::Bits {
        let exp: u32 = 0b1111_1110;
        let frac: u32 = 0b111_1111_1111_1111_1111_1111;

        (u32::from(sign) << 31) + (exp << 23) + frac
    }

    fn normalize(mant: BigFloat) -> (BigFloat, Self::SignedBits) {
        let mut exponent: i32 = 0;
        let limit = BigFloat::from(1.0);
        let multiplier = BigFloat::from(2.0);
        let mut mantissa = mant;
        while mantissa < limit {
            mantissa *= multiplier;
            exponent -= 1;
        }
        while mantissa >= multiplier {
            mantissa /= multiplier;
            exponent += 1;
        }
        (mantissa, exponent)
    }

    fn concate_bits(
        sign: bool,
        biased_exp: Self::SignedBits,
        int_mantissa: Self::Bits,
    ) -> Self::Bits {
        let e = if Self::n() == 32 { 8 } else { 11 };
        let f = Self::n() - e - 1;

        let low: u32 = int_mantissa.get_bits(0..f);
        let mid: u32 = (biased_exp as u32).get_bits(0..e);
        let s = u32::from(sign);

        (s << 31) | (mid << f) | low
    }
}

impl FloatOps for u64 {
    fn native_binary(op: u8, a: u64, b: u64, control: u32) -> Option<(u64, u32)> {
        super::native_float::binary64(op, a, b, control)
    }
    fn native_compare(
        a: u64,
        b: u64,
        control: u32,
        _quiet_nan_exc: bool,
    ) -> Option<((bool, bool, bool, bool), u32)> {
        super::native_float::compare64(a, b, control).map(|flags| (flags, 0))
    }
    type Bits = u64;
    type SignedBits = i64;

    fn fp_infinity(sign: bool) -> Self::Bits {
        if sign {
            0xFFF0_0000_0000_0000
        } else {
            0x7FF0_0000_0000_0000
        }
    }

    fn zero() -> Self::Bits {
        0
    }

    fn n() -> usize {
        64
    }

    fn set_bit(value: Self::Bits, bit: usize, set: bool) -> Self::Bits {
        let mut val = value;
        val.set_bit(bit, set);
        val
    }

    fn integer_to_bits(value: i128) -> Self::Bits {
        value as Self::Bits
    }

    fn powf2(e: BigFloat) -> BigFloat {
        BigFloat::from(2).pow(&e)
    }

    fn unsigned_pow2_f(e: usize) -> BigFloat {
        BigFloat::from(2).pow(&((e as u32).into()))
    }

    fn signed_pow2(e: usize) -> Self::SignedBits {
        2i64.pow(e as u32)
    }

    fn signed_value(value: i32) -> Self::SignedBits {
        i64::from(value)
    }

    fn unsigned_to_signed(value: Self::Bits) -> Self::SignedBits {
        value as Self::SignedBits
    }

    fn bits_to_bigfloat(value: Self::Bits) -> BigFloat {
        BigFloat::from(value)
    }

    fn signedbits_to_bigfloat(value: Self::SignedBits) -> BigFloat {
        BigFloat::from(value)
    }

    fn from_integer(value: u64) -> Self::Bits {
        value
    }

    fn fp_zero(sign: bool) -> Self::Bits {
        if sign {
            0x8000_0000_0000_0000
        } else {
            0x0000_0000_0000_0000
        }
    }

    fn fp_default_nan() -> Self::Bits {
        0x7FF8_0000_0000_0000
    }

    fn is_zero(value: Self::Bits) -> bool {
        value == 0
    }

    fn fp_abs(value: Self::Bits) -> Self::Bits {
        value & 0x7FFF_FFFF_FFFF_FFFF
    }

    fn fp_unpack(fpval: Self::Bits, fpscr_val: u32) -> (FPType, bool, BigFloat, Option<FPExc>) {
        let sign = fpval.get_bit(63);
        let exp64 = fpval.get_bits(52..63);
        let frac64 = fpval.get_bits(0..52);
        let mut exception = None;

        let (ret_type, mut value) = if Self::is_zero(exp64) {
            if Self::is_zero(frac64) || fpscr_val.get_bit(24) {
                if !Self::is_zero(frac64) {
                    exception = Some(FPExc::InputDenorm);
                }
                (FPType::Zero, BigFloat::default())
            } else {
                (
                    FPType::Nonzero,
                    BigFloat::from(2.0).pow(&BigFloat::from(-1022.0))
                        * (BigFloat::from(frac64)
                            * BigFloat::from(2.0).pow(&BigFloat::from(-52.0))),
                )
            }
        } else if is_ones_64(exp64, 11) {
            if Self::is_zero(frac64) {
                (
                    FPType::Infinity,
                    BigFloat::from(2.0).pow(&BigFloat::from(1_000_000.0)),
                )
            } else {
                (
                    if frac64.get_bit(51) {
                        FPType::QNaN
                    } else {
                        FPType::SNaN
                    },
                    BigFloat::default(),
                )
            }
        } else {
            (
                FPType::Nonzero,
                BigFloat::from(2.0).pow(&(BigFloat::from(exp64) - BigFloat::from(1023.0)))
                    * (BigFloat::from(1.0)
                        + BigFloat::from(frac64) * BigFloat::from(2.0).pow(&BigFloat::from(-52.0))),
            )
        };
        if sign {
            value = -value;
        }
        (ret_type, sign, value, exception)
    }

    fn fp_max_normal(sign: bool) -> Self::Bits {
        let exp: u64 = 0b1111_1110;
        let frac: u64 = 0b111_1111_1111_1111_1111_1111_1111_1111_1111_1111;

        (u64::from(sign) << 63) + (exp << 52) + frac
    }

    fn normalize(mant: BigFloat) -> (BigFloat, Self::SignedBits) {
        let mut exponent: i64 = 0;
        let limit = BigFloat::from(1.0);
        let multiplier = BigFloat::from(2.0);
        let mut mantissa = mant;
        while mantissa < limit {
            mantissa *= multiplier;
            exponent -= 1;
        }
        while mantissa >= multiplier {
            mantissa /= multiplier;
            exponent += 1;
        }
        (mantissa, exponent)
    }

    fn concate_bits(
        sign: bool,
        biased_exp: Self::SignedBits,
        int_mantissa: Self::Bits,
    ) -> Self::Bits {
        let e = if Self::n() == 32 { 8 } else { 11 };
        let f = Self::n() - e - 1;
        let low: u64 = int_mantissa.get_bits(0..f);
        let mid: u64 = (biased_exp as u64).get_bits(0..e);
        let s = u64::from(sign);
        (s << 63) | (mid << f) | low
    }
}

fn standard_fpscr_value(fpscr: u32) -> u32 {
    (u32::from(fpscr.get_bit(26)) << 26) | 0b11_0000_0000_0000_0000_0000_0000
}

// Keep the established signed/unsigned bound cases on the production helper.
#[cfg(test)]
fn satq(i: i128, n: usize, unsigned: bool) -> (u64, bool) {
    saturate_magnitude(i.unsigned_abs(), i < 0, n, unsigned)
}

impl FloatingPointHiddenOperations for Processor {
    fn fp_process_nan<T: FloatOps>(
        &mut self,
        type1: FPType,
        op: T::Bits,
        fpscr_val: u32,
    ) -> T::Bits {
        let topfrac = if T::n() == 32 { 22 } else { 51 };
        let mut result = op;
        if type1 == FPType::SNaN {
            result = T::set_bit(result, topfrac, true);
            self.fp_process_exception(FPExc::InvalidOp, fpscr_val);
        }
        if fpscr_val.get_dn() {
            result = T::fp_default_nan();
        }
        result
    }

    fn fp_process_nans<T: FloatOps>(
        &mut self,
        type1: FPType,
        type2: FPType,
        op1: T::Bits,
        op2: T::Bits,
        fpscr_val: u32,
    ) -> (bool, T::Bits) {
        if type1 == FPType::SNaN {
            let result = self.fp_process_nan::<T>(type1, op1, fpscr_val);
            (true, result)
        } else if type2 == FPType::SNaN {
            let result = self.fp_process_nan::<T>(type2, op2, fpscr_val);
            (true, result)
        } else if type1 == FPType::QNaN {
            let result = self.fp_process_nan::<T>(type1, op1, fpscr_val);
            (true, result)
        } else if type2 == FPType::QNaN {
            let result = self.fp_process_nan::<T>(type2, op2, fpscr_val);
            (true, result)
        } else {
            (false, T::zero())
        }
    }

    fn fp_process_nans3<T: FloatOps>(
        &mut self,
        typea: FPType,
        type1: FPType,
        type2: FPType,
        addend: T::Bits,
        op1: T::Bits,
        op2: T::Bits,
        fpscr_val: u32,
    ) -> (bool, T::Bits) {
        if typea == FPType::SNaN {
            (true, self.fp_process_nan::<T>(typea, addend, fpscr_val))
        } else if type1 == FPType::SNaN {
            (true, self.fp_process_nan::<T>(type1, op1, fpscr_val))
        } else if type2 == FPType::SNaN {
            (true, self.fp_process_nan::<T>(type2, op2, fpscr_val))
        } else if typea == FPType::QNaN {
            (true, self.fp_process_nan::<T>(typea, addend, fpscr_val))
        } else if type1 == FPType::QNaN {
            (true, self.fp_process_nan::<T>(type1, op1, fpscr_val))
        } else if type2 == FPType::QNaN {
            (true, self.fp_process_nan::<T>(type2, op2, fpscr_val))
        } else {
            (false, T::zero())
        }
    }

    fn fp_unpack<T: FloatOps>(
        &mut self,
        fpval: T::Bits,
        fpscr_val: u32,
    ) -> (FPType, bool, BigFloat) {
        let (fptype, sign, value, exception) = T::fp_unpack(fpval, fpscr_val);
        if let Some(exc) = exception {
            self.fp_process_exception(exc, fpscr_val);
        }
        (fptype, sign, value)
    }

    fn fp_round<T: FloatOps>(&mut self, value: BigFloat, fpscr_val: u32) -> T::Bits {
        assert!(value != BigFloat::default());

        let e = if T::n() == 32 { 8 } else { 11 };
        let minimum_exp: T::SignedBits = T::signed_value(2) - T::signed_pow2(e - 1);

        let f = T::n() - e - 1;
        let (sign, mantissa) = if value < BigFloat::default() {
            (true, -value)
        } else {
            (false, value)
        };

        let (mut mantissa, exponent) = T::normalize(mantissa);

        if fpscr_val.get_fz() && exponent < minimum_exp {
            self.fpscr.set_ufc(true);
            T::fp_zero(sign)
        } else {
            let mut biased_exp = T::SignedBits::max(
                exponent - minimum_exp + T::signed_value(1),
                T::signed_value(0),
            );
            if biased_exp == T::SignedBits::default() {
                mantissa /= T::powf2((minimum_exp - exponent).into());
            }
            let pow2_f = T::unsigned_pow2_f(f);
            let mut int_mantissa = round_down(mantissa * pow2_f);
            let mut error: BigFloat = mantissa * pow2_f - BigFloat::from(int_mantissa);

            if biased_exp == T::SignedBits::default()
                && (error != BigFloat::default() || fpscr_val.get_bit(11))
            {
                self.fp_process_exception(FPExc::Underflow, fpscr_val);
            }

            let (round_up, overflow_to_inf) = match fpscr_val.get_rounding_mode() {
                FPSCRRounding::RoundToNearest => (
                    (error > BigFloat::from(0.5)
                        || (error == BigFloat::from(0.5) && int_mantissa.get_bit(0))),
                    true,
                ),
                FPSCRRounding::RoundTowardsPlusInfinity => {
                    (error != BigFloat::default() && !sign, !sign)
                }
                FPSCRRounding::RoundTowardsMinusInfinity => {
                    (error != BigFloat::default() && sign, sign)
                }
                FPSCRRounding::RoundTowardsZero => (false, false),
            };
            if round_up {
                int_mantissa += 1;
                if int_mantissa == 2i128.pow(f as u32) {
                    biased_exp = T::signed_value(1);
                }
                if int_mantissa == 2i128.pow(f as u32 + 1) {
                    biased_exp += T::signed_value(1);
                    int_mantissa /= 2;
                }
            }

            let result = if biased_exp >= T::signed_pow2(e) - T::signed_value(1) {
                let result = if overflow_to_inf {
                    T::fp_infinity(sign)
                } else {
                    T::fp_max_normal(sign)
                };
                self.fp_process_exception(FPExc::Overflow, fpscr_val);
                error = num_bigfloat::ONE;
                result
            } else {
                // concatenate following bits:
                // sign: biased_exp(0..=e-1) : int_mant(0..=f-1)
                T::concate_bits(sign, biased_exp, T::integer_to_bits(int_mantissa))
            };

            if error != BigFloat::default() {
                self.fp_process_exception(FPExc::Inexact, fpscr_val);
            }
            result
        }
    }
}

impl FloatingPointPublicOperations for Processor {
    fn fp_minmax_num<T: FloatOps>(&mut self, op1: T::Bits, op2: T::Bits, maximum: bool) -> T::Bits {
        let control = self.fpscr;
        let sign = 1u64 << (T::n() - 1);
        let exponent: u64 = T::fp_infinity(false).into();
        let quiet = 1u64 << (if T::n() == 32 { 22 } else { 51 });
        let mut a: u64 = op1.into();
        let mut b: u64 = op2.into();
        if control & (1 << 24) != 0 {
            for operand in [&mut a, &mut b] {
                let magnitude = *operand & !sign;
                if magnitude != 0 && magnitude < quiet * 2 {
                    *operand &= sign;
                    self.fp_process_exception(FPExc::InputDenorm, control);
                }
            }
        }
        let nan_a = a & !sign > exponent;
        let nan_b = b & !sign > exponent;
        let signal_a = nan_a && a & quiet == 0;
        let signal_b = nan_b && b & quiet == 0;
        if signal_a || signal_b || (nan_a && nan_b) {
            let (bits, kind) = if signal_a {
                (a, FPType::SNaN)
            } else if signal_b {
                (b, FPType::SNaN)
            } else {
                (a, FPType::QNaN)
            };
            return self.fp_process_nan::<T>(kind, T::from_integer(bits), control);
        }
        // A lone quiet NaN contributes no number; the other operand wins.
        if nan_a {
            return T::from_integer(b);
        }
        if nan_b {
            return T::from_integer(a);
        }
        if (a | b) & !sign == 0 {
            return T::from_integer(if maximum { a & b } else { a | b });
        }
        // IEEE bit ordering is monotonic within each sign. This preserves
        // exact subnormals/infinities without host NaN or rounding behavior.
        let greater = if (a ^ b) & sign != 0 {
            a & sign == 0
        } else if a & sign != 0 {
            a < b
        } else {
            a > b
        };
        T::from_integer(if greater == maximum { a } else { b })
    }
    fn fp_add<T: FloatOps>(
        &mut self,
        op1: T::Bits,
        op2: T::Bits,
        fpscr_controlled: bool,
    ) -> T::Bits {
        let fpscr_val = if fpscr_controlled {
            self.fpscr
        } else {
            standard_fpscr_value(self.fpscr)
        };
        if self.native_float {
            if let Some((bits, flags)) = T::native_binary(0, op1, op2, fpscr_val) {
                self.fpscr |= flags;
                return bits;
            }
        }
        self.fp_binary_ieee::<T>(0, op1, op2, fpscr_val)
    }

    fn fp_sub<T: FloatOps>(
        &mut self,
        op1: T::Bits,
        op2: T::Bits,
        fpscr_controlled: bool,
    ) -> T::Bits {
        let fpscr_val = if fpscr_controlled {
            self.fpscr
        } else {
            standard_fpscr_value(self.fpscr)
        };
        if self.native_float {
            if let Some((bits, flags)) = T::native_binary(1, op1, op2, fpscr_val) {
                self.fpscr |= flags;
                return bits;
            }
        }
        self.fp_binary_ieee::<T>(1, op1, op2, fpscr_val)
    }

    fn fp_mul_add<T: FloatOps>(
        &mut self,
        addend: T::Bits,
        op1: T::Bits,
        op2: T::Bits,
        fpscr_controlled: bool,
    ) -> T::Bits {
        let fpscr_val = if fpscr_controlled {
            self.fpscr
        } else {
            standard_fpscr_value(self.fpscr)
        };

        let (typea, signa) = integer_operand(self, addend.into(), T::n(), fpscr_val);
        let (type1, sign1) = integer_operand(self, op1.into(), T::n(), fpscr_val);
        let (type2, sign2) = integer_operand(self, op2.into(), T::n(), fpscr_val);

        let inf1 = type1 == FPType::Infinity;
        let zero1 = type1 == FPType::Zero;
        let inf2 = type2 == FPType::Infinity;
        let zero2 = type2 == FPType::Zero;

        let (done, mut result) =
            self.fp_process_nans3::<T>(typea, type1, type2, addend, op1, op2, fpscr_val);

        if typea == FPType::QNaN && ((inf1 && zero2) || (zero1 && inf2)) {
            result = T::fp_default_nan();
            self.fp_process_exception(FPExc::InvalidOp, fpscr_val);
            return result;
        }

        if done {
            return result;
        }

        let a = if zero1 { T::fp_zero(sign1) } else { op1 };
        let b = if zero2 { T::fp_zero(sign2) } else { op2 };
        let c = if typea == FPType::Zero {
            T::fp_zero(signa)
        } else {
            addend
        };
        let (bits, flags) = super::fp_ieee::fused(a.into(), b.into(), c.into(), T::n(), fpscr_val);
        self.fpscr |= flags;
        if flags & 1 != 0 {
            T::fp_default_nan()
        } else {
            T::from_integer(bits)
        }
    }

    fn fp_mul<T: FloatOps>(
        &mut self,
        op1: T::Bits,
        op2: T::Bits,
        fpscr_controlled: bool,
    ) -> T::Bits {
        let fpscr_val = if fpscr_controlled {
            self.fpscr
        } else {
            standard_fpscr_value(self.fpscr)
        };

        if self.native_float {
            if let Some((bits, flags)) = T::native_binary(2, op1, op2, fpscr_val) {
                self.fpscr |= flags;
                return bits;
            }
        }
        self.fp_binary_ieee::<T>(2, op1, op2, fpscr_val)
    }

    fn fp_div<T: FloatOps>(
        &mut self,
        op1: T::Bits,
        op2: T::Bits,
        fpscr_controlled: bool,
    ) -> T::Bits {
        let fpscr_val = if fpscr_controlled {
            self.fpscr
        } else {
            standard_fpscr_value(self.fpscr)
        };

        if self.native_float {
            if let Some((bits, flags)) = T::native_binary(3, op1, op2, fpscr_val) {
                self.fpscr |= flags;
                return bits;
            }
        }
        self.fp_binary_ieee::<T>(3, op1, op2, fpscr_val)
    }

    fn fp_compare<T: FloatOps>(
        &mut self,
        op1: T::Bits,
        op2: T::Bits,
        quiet_nan_exc: bool,
        fpscr_controlled: bool,
    ) -> (bool, bool, bool, bool) {
        let fpscr_val = if fpscr_controlled {
            self.fpscr
        } else {
            standard_fpscr_value(self.fpscr)
        };

        if self.native_float {
            if let Some((flags, status)) = T::native_compare(op1, op2, fpscr_val, quiet_nan_exc) {
                self.fpscr |= status;
                return flags;
            }
        }
        let (type1, _sign1, value1) = self.fp_unpack::<T>(op1, fpscr_val);
        let (type2, _sign2, value2) = self.fp_unpack::<T>(op2, fpscr_val);

        if type1 == FPType::SNaN
            || type1 == FPType::QNaN
            || type2 == FPType::SNaN
            || type2 == FPType::QNaN
        {
            let result = (false, false, true, true);
            if type1 == FPType::SNaN || type2 == FPType::SNaN || quiet_nan_exc {
                self.fp_process_exception(FPExc::InvalidOp, fpscr_val);
            }
            result
        } else if value1 == value2 {
            (false, true, true, false)
        } else if value1 < value2 {
            (true, false, false, false)
        } else {
            (false, false, true, false)
        }
    }

    fn fp_abs<T: FloatOps>(&mut self, op: T::Bits) -> T::Bits {
        T::fp_abs(op)
    }

    fn fp_sqrt<T: FloatOps>(&mut self, op: T::Bits, fpscr_controlled: bool) -> T::Bits {
        let fpscr_val = if fpscr_controlled {
            self.fpscr
        } else {
            standard_fpscr_value(self.fpscr)
        };

        if self.native_float && T::n() == 32 {
            if let Some((bits, flags)) =
                T::as_u32(op).and_then(|input| super::native_float::square_root(input, fpscr_val))
            {
                self.fpscr |= flags;
                return T::from_u32(bits).unwrap();
            }
        }

        let (fptype, sign) = integer_operand(self, op.into(), T::n(), fpscr_val);

        match fptype {
            FPType::SNaN | FPType::QNaN => self.fp_process_nan::<T>(fptype, op, fpscr_val),
            FPType::Infinity => {
                if sign {
                    self.fp_process_exception(FPExc::InvalidOp, fpscr_val);
                    T::fp_default_nan()
                } else {
                    T::fp_infinity(false)
                }
            }
            FPType::Zero => T::fp_zero(sign),
            FPType::Nonzero => {
                if sign {
                    self.fp_process_exception(FPExc::InvalidOp, fpscr_val);
                    return T::fp_default_nan();
                }

                let (bits, flags) = super::fp_ieee::square_root(op.into(), T::n(), fpscr_val);
                self.fpscr |= flags;
                T::from_integer(bits)
            }
        }
    }

    fn fp_round_int<T: FloatOps>(
        &mut self,
        op: T::Bits,
        zero_rounding: bool,
        exact: bool,
        fpscr_controlled: bool,
    ) -> T::Bits {
        self.fp_round_int_with_rounding::<T>(
            op,
            zero_rounding.then_some(IntegralRounding::TowardZero),
            exact,
            fpscr_controlled,
        )
    }

    fn fp_round_int_with_rounding<T: FloatOps>(
        &mut self,
        op: T::Bits,
        rounding: Option<IntegralRounding>,
        exact: bool,
        fpscr_controlled: bool,
    ) -> T::Bits {
        let fpscr_val = if fpscr_controlled {
            self.fpscr
        } else {
            standard_fpscr_value(self.fpscr)
        };

        let rounding = rounding.unwrap_or_else(|| IntegralRounding::from_fpscr(fpscr_val));

        let (fptype, sign) = integer_operand(self, op.into(), T::n(), fpscr_val);

        if fptype == FPType::SNaN || fptype == FPType::QNaN {
            return self.fp_process_nan::<T>(fptype, op, fpscr_val);
        }

        if fptype == FPType::Infinity {
            return T::fp_infinity(sign);
        }

        if fptype == FPType::Zero {
            return T::fp_zero(sign);
        }

        // Every finite binary32 value at |x| >= 2^23 (binary64: 2^52) is
        // already integral. Returning its bits also avoids clipping to i128.
        let bits: u64 = op.into();
        let (fraction, bias, exponent_mask) = ieee_layout(T::n());
        if ((bits >> fraction) & exponent_mask) >= (bias as u32 + fraction) as u64 {
            return op;
        }
        let (magnitude, fractional) = rounded_magnitude(bits, T::n(), 0, rounding);
        let result = if magnitude == 0 {
            T::fp_zero(sign)
        } else {
            let exponent = 127 - magnitude.leading_zeros();
            let mantissa = ((magnitude << (fraction - exponent)) as u64) & ((1u64 << fraction) - 1);
            T::from_integer(
                (u64::from(sign) << (T::n() - 1))
                    | ((u64::from(exponent) + bias as u64) << fraction)
                    | mantissa,
            )
        };

        if fractional && exact {
            self.fp_process_exception(FPExc::Inexact, fpscr_val);
        }

        result
    }

    fn fixed_to_fp<N: FloatOps, M: FloatOps>(
        &mut self,
        op: M::Bits,
        fraction_bits: usize,
        unsigned: bool,
        round_to_nearest: bool,
        fpscr_controlled: bool,
    ) -> N::Bits {
        let mut fpscr_val = if fpscr_controlled {
            self.fpscr
        } else {
            standard_fpscr_value(self.fpscr)
        };
        if round_to_nearest {
            fpscr_val.set_rounding_mode(FPSCRRounding::RoundToNearest);
        }

        if self.native_float
            && N::n() == 32
            && M::n() == 32
            && fraction_bits == 0
            && fpscr_val & 0x00c09f00 == 0
        {
            if let Some(bits) = M::as_u32(op) {
                let exact = if unsigned {
                    f64::from(bits)
                } else {
                    f64::from(bits as i32)
                };
                let result = exact as f32;
                if f64::from(result) != exact {
                    self.fpscr |= 16;
                }
                return N::from_u32(result.to_bits()).unwrap();
            }
        }
        let real_operand = if unsigned {
            let int_operand = op;
            BigFloat::div(
                &M::bits_to_bigfloat(int_operand),
                &M::powf2(BigFloat::from(fraction_bits as u32)),
            )
        } else {
            let int_operand = M::unsigned_to_signed(op);
            BigFloat::div(
                &M::signedbits_to_bigfloat(int_operand),
                &M::powf2(BigFloat::from(fraction_bits as u32)),
            )
        };

        if real_operand == BigFloat::default() {
            N::zero()
        } else {
            self.fp_round::<N>(real_operand, fpscr_val)
        }
    }

    fn fp_to_fixed<N: FloatOps, M: FloatOps>(
        &mut self,
        op: N::Bits,
        fraction_bits: usize,
        unsigned: bool,
        round_towards_zero: bool,
        fpscr_controlled: bool,
    ) -> M::Bits {
        self.fp_to_fixed_with_rounding::<N, M>(
            op,
            fraction_bits,
            unsigned,
            round_towards_zero.then_some(IntegralRounding::TowardZero),
            fpscr_controlled,
        )
    }

    fn fp_to_fixed_with_rounding<N: FloatOps, M: FloatOps>(
        &mut self,
        op: N::Bits,
        fraction_bits: usize,
        unsigned: bool,
        rounding: Option<IntegralRounding>,
        fpscr_controlled: bool,
    ) -> M::Bits {
        let fpscr_val = if fpscr_controlled {
            self.fpscr
        } else {
            standard_fpscr_value(self.fpscr)
        };

        let rounding = rounding.unwrap_or_else(|| IntegralRounding::from_fpscr(fpscr_val));
        M::from_integer(float_to_fixed_bits(
            self,
            op.into(),
            N::n(),
            M::n(),
            fraction_bits,
            unsigned,
            rounding,
            fpscr_val,
        ))
    }
}

fn is_ones_32(value: u32, len: usize) -> bool {
    value.count_ones() == len as u32
}

fn is_ones_64(value: u64, len: usize) -> bool {
    value.count_ones() == len as u32
}

impl FloatingPointChecks for Processor {
    #[inline(always)]
    fn check_vfp_enabled(&mut self) -> Result<(), Fault> {
        let cp10 = self.cpacr.get_bits(20..22);
        let cp11 = self.cpacr.get_bits(22..24);

        if cp10 != cp11 {
            return Err(Fault::Forced);
        }

        match cp10 {
            0b00 => {
                self.cfsr.set_bit(19, true);
                Err(Fault::Nocp)
            }
            0b01 => {
                if !self.current_mode_is_privileged() {
                    self.cfsr.set_bit(19, true);
                    Err(Fault::Nocp)
                } else {
                    Ok(())
                }
            }
            0b10 => {
                self.cfsr.set_bit(19, true);
                Err(Fault::Nocp)
            }
            0b11 => Ok(()), // full access
            _ => unreachable!(),
        }
    }
    #[inline(always)]
    fn execute_fp_check(&mut self) -> Result<(), Fault> {
        self.check_vfp_enabled()?;

        if self.fpccr.get_bit(FPCCR_LSPACT) {
            self.preserve_fp_state()?;
        }

        if self.fpccr.get_bit(FPCCR_ASPEN) && !self.control.fpca {
            let fpdscr_bits = self
                .fpdscr
                .get_bits(FPSCR_STATUS_CONTROL_START..FPSCR_STATUS_CONTROL_END);
            self.fpscr.set_bits(
                FPSCR_STATUS_CONTROL_START..FPSCR_STATUS_CONTROL_END,
                fpdscr_bits,
            );
            self.control.fpca = true;
        }
        Ok(())
    }

    fn fp_process_exception(&mut self, exc: FPExc, fpscr_val: u32) {
        let (enable, cumul) = match exc {
            FPExc::InvalidOp => (8, 0),
            FPExc::DivideByZero => (9, 1),
            FPExc::Overflow => (10, 2),
            FPExc::Underflow => (11, 3),
            FPExc::Inexact => (12, 4),
            FPExc::InputDenorm => (15, 7),
        };
        if fpscr_val.get_bit(enable) {
            // implementation defined trap handling
            todo!()
        } else {
            self.fpscr.set_bit(cumul, true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Processor;
    use crate::bus::Bus;
    use crate::core::bits::Bits;
    use crate::core::fault::Fault;
    use crate::core::register::{ExtensionRegOperations, SingleReg};

    #[test]
    fn test_fp_compare_f32() {
        let mut processor = Processor::new();
        assert_eq!(
            processor.fp_compare::<u32>(0x3F80_0000, 0x3F80_0000, false, false),
            (false, true, true, false)
        ); // 1.0 == 1.0
        assert_eq!(
            processor.fp_compare::<u32>(0x3F80_0000, 0x4000_0000, false, false),
            (true, false, false, false)
        ); // 1.0 < 2.0
        assert_eq!(
            processor.fp_compare::<u32>(0x4000_0000, 0x3F80_0000, false, false),
            (false, false, true, false)
        ); // 2.0 > 1.0
    }

    #[test]
    fn test_fp_compare_f64() {
        let mut processor = Processor::new();

        // 1.0 == 1.0
        assert_eq!(
            processor.fp_compare::<u64>(0x3FF0_0000_0000_0000, 0x3FF0_0000_0000_0000, false, false),
            (false, true, true, false)
        );

        // 1.0 < 2.0
        assert_eq!(
            processor.fp_compare::<u64>(0x3FF0_0000_0000_0000, 0x4000_0000_0000_0000, false, false),
            (true, false, false, false)
        );
        // 2.0 > 1.0
        assert_eq!(
            processor.fp_compare::<u64>(0x4000_0000_0000_0000, 0x3FF0_0000_0000_0000, false, false),
            (false, false, true, false)
        );
    }

    #[test]
    fn test_fpabs_32() {
        let mut processor = Processor::new();

        // -0.0 -> 0.0
        assert_eq!(processor.fp_abs::<u32>(0x8000_0000), 0x0000_0000);

        // -1.0 -> 1.0
        assert_eq!(processor.fp_abs::<u32>(0xFFFF_FFFF), 0x7FFF_FFFF);

        // 1.0 -> 1.0
        assert_eq!(processor.fp_abs::<u32>(0x7FFF_FFFF), 0x7FFF_FFFF);

        // most negative value:
        assert_eq!(
            processor.fp_abs::<u32>((-f32::MAX).to_bits()),
            f32::MAX.to_bits()
        );
    }
    #[test]
    fn test_fpabs_64() {
        let mut processor = Processor::new();
        // -0.0 -> 0.0
        assert_eq!(
            processor.fp_abs::<u64>(0x8000_0000_0000_0000),
            0x0000_0000_0000_0000
        );
        // -1.0 -> 1.0
        assert_eq!(
            processor.fp_abs::<u64>(0xFFFF_FFFF_FFFF_FFFF),
            0x7FFF_FFFF_FFFF_FFFF
        );
        // 1.0 -> 1.0
        assert_eq!(
            processor.fp_abs::<u64>(0x7FFF_FFFF_FFFF_FFFF),
            0x7FFF_FFFF_FFFF_FFFF
        );

        // min value
        assert_eq!(
            processor.fp_abs::<u64>((-f64::MAX).to_bits()),
            f64::MAX.to_bits()
        );
    }

    #[test]
    fn test_fp_unpack_f32() {
        let mut processor = Processor::new();

        // unpack extracts the sign, exponent and mantissa from a 32-bit float u32 represenantation

        // 1.0
        assert_eq!(
            processor.fp_unpack::<u32>(0x3F80_0000, 0x0000_0000),
            (FPType::Nonzero, false, BigFloat::from(1.0))
        );

        // -1.0
        assert_eq!(
            processor.fp_unpack::<u32>(0xBF80_0000, 0x0000_0000),
            (FPType::Nonzero, true, BigFloat::from(-1.0))
        );

        // 2.0
        assert_eq!(
            processor.fp_unpack::<u32>(0x4000_0000, 0x0000_0000),
            (FPType::Nonzero, false, BigFloat::from(2.0))
        );

        // max value
        assert_eq!(
            processor.fp_unpack::<u32>(0x7F7F_FFFF, 0x0000_0000),
            (
                FPType::Nonzero,
                false,
                BigFloat::from(340_282_346_638_528_859_811_704_183_484_516_925_440_u128)
            )
        );

        // 0.0
        assert_eq!(
            processor.fp_unpack::<u32>(0x0000_0000, 0x0000_0000),
            (FPType::Zero, false, BigFloat::default())
        );

        // minimum positive value, non zero:
        assert_eq!(
            processor.fp_unpack::<u32>(0x0080_0000, 0x0000_0000),
            (
                FPType::Nonzero,
                false,
                BigFloat::parse("1.175494350822287507968736537222245677819e-38").unwrap()
            )
        );

        // Infinity
        assert_eq!(
            processor.fp_unpack::<u32>(0x7F80_0000, 0x0000_0000),
            (FPType::Infinity, false, num_bigfloat::INF_POS)
        );

        // Negative infinity:
        assert_eq!(
            processor.fp_unpack::<u32>(0xFF80_0000, 0x0000_0000),
            (FPType::Infinity, true, num_bigfloat::INF_NEG)
        );

        // QNaN
        assert_eq!(
            processor.fp_unpack::<u32>(0x7FC0_0000, 0x0000_0000),
            (FPType::QNaN, false, BigFloat::default())
        );

        // SNaN
        assert_eq!(
            processor.fp_unpack::<u32>(0x7F80_0001, 0x0000_0000),
            (FPType::SNaN, false, BigFloat::default())
        );
    }

    #[test]
    fn test_fp_unpack_f64() {
        let mut processor = Processor::new();

        // 1.0
        assert_eq!(
            processor.fp_unpack::<u64>(0x3FF0_0000_0000_0000, 0x0000_0000),
            (FPType::Nonzero, false, BigFloat::from(1.0))
        );

        // -1.0
        assert_eq!(
            processor.fp_unpack::<u64>(0xBFF0_0000_0000_0000, 0x0000_0000),
            (FPType::Nonzero, true, BigFloat::from(-1.0))
        );

        // 2.0
        assert_eq!(
            processor.fp_unpack::<u64>(0x4000_0000_0000_0000, 0x0000_0000),
            (FPType::Nonzero, false, BigFloat::from(2.0))
        );

        // max value:
        assert_eq!(
            processor.fp_unpack::<u64>(0x7FEF_FFFF_FFFF_FFFF, 0x0000_0000),
            (FPType::Nonzero, false, BigFloat::from(f64::MAX))
        );

        // 0.0
        assert_eq!(
            processor.fp_unpack::<u64>(0x0000_0000_0000_0000, 0x0000_0000),
            (FPType::Zero, false, BigFloat::default())
        );

        // minimum positive value, non zero:
        assert_eq!(
            processor.fp_unpack::<u64>(0x0010_0000_0000_0000, 0x0000_0000),
            (FPType::Nonzero, false, BigFloat::from(f64::MIN_POSITIVE))
        );

        // Infinity
        assert_eq!(
            processor.fp_unpack::<u64>(0x7FF0_0000_0000_0000, 0x0000_0000),
            (FPType::Infinity, false, num_bigfloat::INF_POS)
        );

        // Negative infinity:
        assert_eq!(
            processor.fp_unpack::<u64>(0xFFF0_0000_0000_0000, 0x0000_0000),
            (FPType::Infinity, true, num_bigfloat::INF_NEG)
        );

        // QNaN
        assert_eq!(
            processor.fp_unpack::<u64>(0x7FF8_0000_0000_0000, 0x0000_0000),
            (FPType::QNaN, false, BigFloat::default())
        );

        // SNaN
        assert_eq!(
            processor.fp_unpack::<u64>(0x7FF0_0000_0000_0001, 0x0000_0000),
            (FPType::SNaN, false, BigFloat::default())
        );
    }
    #[test]
    fn test_fp_round_f32() {
        let mut processor = Processor::new();

        // Test all rounding modes:
        // FPSCRRounding::RoundToNearest
        // FPSCRRounding::RoundTowardsPlusInfinity
        // FPSCRRounding::RoundTowardsMinusInfinity
        // FPSCRRounding::RoundTowardsZero,

        // Default rounding mode
        processor
            .fpscr
            .set_rounding_mode(FPSCRRounding::RoundToNearest);

        // 1.0 -> 0x3F80_0000 exact
        assert_eq!(
            processor.fp_round::<u32>(BigFloat::from(1.0), 0),
            0x3F80_0000
        );

        // 0.33333333333333333333 -> 3eaaaaab
        assert_eq!(
            processor.fp_round::<u32>(BigFloat::from(0.333_333_333_333_333_3), 0),
            0x3eaa_aaab
        );
    }

    #[test]
    fn test_fp_add_f32() {
        let mut processor = Processor::new();

        // 1.0 + 1.0 = 2.0
        assert_eq!(
            processor.fp_add::<u32>(0x3F80_0000, 0x3F80_0000, true),
            0x4000_0000
        );

        // 1.0 + 2.0 = 3.0
        assert_eq!(
            processor.fp_add::<u32>(0x3F80_0000, 0x4000_0000, true),
            0x4040_0000
        );

        // -1.0 + 2.0 = 1.0
        assert_eq!(
            processor.fp_add::<u32>(0xBF80_0000, 0x4000_0000, true),
            0x3F80_0000
        );

        // 0.9038018 + epsilon should round back to the original sigma value.
        assert_eq!(
            processor.fp_add::<u32>(0x3F67_5F8E, 0x3194_BDC1, true),
            0x3F67_5F8E
        );
    }

    #[test]
    fn test_fp_sub_f32() {
        let mut processor = Processor::new();

        // 1.0 - 1.0 = 0.0
        assert_eq!(
            processor.fp_sub::<u32>(0x3F80_0000, 0x3F80_0000, true),
            0x0000_0000
        );

        // 2.0 - 1.0 = 1.0
        assert_eq!(
            processor.fp_sub::<u32>(0x4000_0000, 0x3F80_0000, true),
            0x3F80_0000
        );

        // 1.0 - 2.0 = -1.0
        assert_eq!(
            processor.fp_sub::<u32>(0x3F80_0000, 0x4000_0000, true),
            0xBF80_0000
        );

        // 1.5 - 1.4539529 should preserve the small positive difference.
        assert_eq!(
            processor.fp_sub::<u32>(0x3FC0_0000, 0x3FBA_1B21, true),
            0x3D3C_9BE0
        );
    }

    #[test]
    fn test_fp_mul_add_f32_basic() {
        let mut processor = Processor::new();

        assert_eq!(
            processor.fp_mul_add::<u32>(1.0f32.to_bits(), 2.0f32.to_bits(), 3.0f32.to_bits(), true),
            7.0f32.to_bits()
        );
    }

    #[test]
    fn test_fp_mul_add_zero_sign_uses_rounding_mode() {
        let mut processor = Processor::new();
        processor
            .fpscr
            .set_rounding_mode(FPSCRRounding::RoundTowardsMinusInfinity);

        let result = processor.fp_mul_add::<u32>(
            (-0.0f32).to_bits(),
            0.0f32.to_bits(),
            5.0f32.to_bits(),
            true,
        );

        assert_eq!(result, (-0.0f32).to_bits());
    }

    #[test]
    fn test_fp_mul_add_inf_times_zero_is_invalid() {
        let mut processor = Processor::new();
        processor.fpscr = 0;

        let result = processor.fp_mul_add::<u32>(
            1.0f32.to_bits(),
            f32::INFINITY.to_bits(),
            0.0f32.to_bits(),
            true,
        );

        assert_eq!(result, <u32 as FloatOps>::fp_default_nan());
        assert!(processor.fpscr.get_bit(0));
    }

    #[test]
    fn test_fp_mul_f32_basic() {
        let mut processor = Processor::new();

        assert_eq!(
            processor.fp_mul::<u32>(2.0f32.to_bits(), 3.0f32.to_bits(), true),
            6.0f32.to_bits()
        );
    }

    #[test]
    fn test_fp_div_f32_basic() {
        let mut processor = Processor::new();

        assert_eq!(
            processor.fp_div::<u32>(6.0f32.to_bits(), 2.0f32.to_bits(), true),
            3.0f32.to_bits()
        );
    }

    #[test]
    fn test_fp_div_f32_by_zero_sets_exception() {
        let mut processor = Processor::new();
        processor.fpscr = 0;

        let result = processor.fp_div::<u32>(1.0f32.to_bits(), 0.0f32.to_bits(), true);

        assert_eq!(result, f32::INFINITY.to_bits());
        assert!(processor.fpscr.get_bit(1));
    }

    #[test]
    fn test_fp_mul_add_qnan_addend_still_reports_invalid_for_inf_times_zero() {
        let mut processor = Processor::new();
        processor.fpscr = 0;

        let result = processor.fp_mul_add::<u32>(
            0x7fc0_1234,
            f32::INFINITY.to_bits(),
            0.0f32.to_bits(),
            true,
        );

        assert_eq!(result, <u32 as FloatOps>::fp_default_nan());
        assert!(processor.fpscr.get_bit(0));
    }

    #[test]
    fn test_fp_sqrt_f32_round_towards_zero_uses_helper_rounding() {
        let mut processor = Processor::new();
        processor
            .fpscr
            .set_rounding_mode(FPSCRRounding::RoundTowardsZero);

        let op = 2.0f32.to_bits();
        let result = processor.fp_sqrt::<u32>(op, true);

        let mut expected_processor = Processor::new();
        expected_processor
            .fpscr
            .set_rounding_mode(FPSCRRounding::RoundTowardsZero);
        let expected = expected_processor
            .fp_round::<u32>(BigFloat::from(2.0).sqrt(), expected_processor.fpscr);

        assert_eq!(result, expected);
        assert!(processor.fpscr.get_bit(4));
    }

    #[test]
    fn test_fp_sqrt_f64_round_towards_plus_infinity_uses_helper_rounding() {
        let mut processor = Processor::new();
        processor
            .fpscr
            .set_rounding_mode(FPSCRRounding::RoundTowardsPlusInfinity);

        let op = 2.0f64.to_bits();
        let result = processor.fp_sqrt::<u64>(op, true);

        let mut expected_processor = Processor::new();
        expected_processor
            .fpscr
            .set_rounding_mode(FPSCRRounding::RoundTowardsPlusInfinity);
        let expected = expected_processor
            .fp_round::<u64>(BigFloat::from(2.0).sqrt(), expected_processor.fpscr);

        assert_eq!(result, expected);
        assert!(processor.fpscr.get_bit(4));
    }

    #[test]
    fn test_fp_round_int_zero_rounding_ignores_fpscr_round_mode() {
        let mut processor = Processor::new();
        processor
            .fpscr
            .set_rounding_mode(FPSCRRounding::RoundTowardsPlusInfinity);

        let op = (-2.9f32).to_bits();
        let result = processor.fp_round_int::<u32>(op, true, false, true);

        assert_eq!(result, (-2.0f32).to_bits());
    }

    #[test]
    fn test_fp_round_int_exact_sets_inexact() {
        let mut processor = Processor::new();
        processor.fpscr = 0;

        let op = 1.25f32.to_bits();
        let _ = processor.fp_round_int::<u32>(op, true, true, true);

        assert!(processor.fpscr.get_bit(4));
    }

    #[test]
    fn test_fp_round_int_exact_false_does_not_set_inexact() {
        let mut processor = Processor::new();
        processor.fpscr = 0;

        let op = 1.25f32.to_bits();
        let _ = processor.fp_round_int::<u32>(op, true, false, true);

        assert!(!processor.fpscr.get_bit(4));
    }

    #[test]
    fn test_fp_round_int_keeps_infinity() {
        let mut processor = Processor::new();

        let pos = processor.fp_round_int::<u32>(f32::INFINITY.to_bits(), true, false, true);
        let neg = processor.fp_round_int::<u32>(f32::NEG_INFINITY.to_bits(), true, false, true);

        assert_eq!(pos, f32::INFINITY.to_bits());
        assert_eq!(neg, f32::NEG_INFINITY.to_bits());
    }

    #[test]
    fn test_fp_round_int_preserves_signed_zero() {
        let mut processor = Processor::new();

        let pos = processor.fp_round_int::<u32>(0.0f32.to_bits(), true, false, true);
        let neg = processor.fp_round_int::<u32>((-0.0f32).to_bits(), true, false, true);

        assert_eq!(pos, 0.0f32.to_bits());
        assert_eq!(neg, (-0.0f32).to_bits());
    }

    #[test]
    fn test_fp_round_int_uses_fpscr_mode_when_zero_rounding_is_false() {
        let mut processor = Processor::new();
        processor
            .fpscr
            .set_rounding_mode(FPSCRRounding::RoundTowardsPlusInfinity);

        let op = 2.1f32.to_bits();
        let result = processor.fp_round_int::<u32>(op, false, false, true);

        assert_eq!(result, 3.0f32.to_bits());
    }

    #[test]
    fn test_fp_round_f64() {
        let mut processor = Processor::new();

        // 1.0 -> 0x3FF0000000000000 exact
        assert_eq!(
            processor.fp_round::<u64>(BigFloat::from(1.0f64), 0),
            0x3FF0_0000_0000_0000
        );
    }

    #[test]
    fn test_satq() {
        // 1.0 -> 1 (unsigned, 32 bit)
        assert_eq!(satq(1, 32, true), (1, false));

        // 1.0 -> 1 (signed, 32 bit)
        assert_eq!(satq(1, 32, false), (1, false));

        // 1.0 -> 1 (unsigned, 64 bit)
        assert_eq!(satq(1, 64, true), (1, false));

        // 1.0 -> 1 (signed, 64 bit)
        assert_eq!(satq(1, 64, false), (1, false));

        // -1.0 -> 0 (signed, 32 bit)
        assert_eq!(satq(-1, 32, false), (0xffff_ffff, false));

        // value bigger than max value (unsigned, 32 bit)
        assert_eq!(satq(0x1_0000_0000, 32, true), (0xffff_ffff, true));

        // value bigger than max value (signed, 32 bit)
        assert_eq!(satq(0x8000_0000, 32, false), (0x7fff_ffff, true));

        // value smaller than min value (signed, 32 bit)
        assert_eq!(satq(-0x8000_0001, 32, false), (0x8000_0000, true));

        // value bigger than max value (unsigned, 64 bit)
        assert_eq!(
            satq(0x1_0000_0000_0000_0000, 64, true),
            (0xffff_ffff_ffff_ffff, true)
        );

        // value bigger than max value (signed, 64 bit)
        assert_eq!(
            satq(0x8000_0000_0000_0000, 64, false),
            (0x7fff_ffff_ffff_ffff, true)
        );

        // value smaller than min value (signed, 64 bit)
        assert_eq!(
            satq(-0x8000_0000_0000_0001, 64, false),
            (0x8000_0000_0000_0000, true)
        );

        // negative value (unsigned, 64 bit)
        assert_eq!(satq(-1, 64, true), (0, true));
    }

    #[test]
    fn test_fp_to_fixed_f32_s32() {
        let mut processor = Processor::new();

        // 1.0 -> 1 (signed)
        assert_eq!(
            processor.fp_to_fixed::<u32, u32>(0x3F80_0000, 0, false, false, false),
            0x0000_0001
        );

        // 0.00001 -> 0 (signed) (rounding towards zero)
        assert_eq!(
            processor.fp_to_fixed::<u32, u32>(0x3C23_D70A, 0, false, false, true),
            0x0000_0000
        );

        // 42.0 -> 42 (signed)
        assert_eq!(
            processor.fp_to_fixed::<u32, u32>(0x4228_0000, 0, false, false, false),
            0x0000_002A
        );

        // -1.0 -> -1 (signed)
        assert_eq!(
            processor.fp_to_fixed::<u32, u32>(0xBF80_0000, 0, false, false, false),
            0xFFFF_FFFF
        );

        // -42.0 -> -42 (signed)
        assert_eq!(
            processor.fp_to_fixed::<u32, u32>(0xC228_0000, 0, false, false, false),
            0xFFFF_FFD6
        );

        // positive infinity -> max value
        assert_eq!(
            processor.fp_to_fixed::<u32, u32>(0x7F80_0000, 0, false, false, false),
            0x7FFF_FFFF
        );

        // negative infinity -> min value
        assert_eq!(
            processor.fp_to_fixed::<u32, u32>(0xFF80_0000, 0, false, false, false),
            0x8000_0000
        );

        // QNAN -> 0
        assert_eq!(
            processor.fp_to_fixed::<u32, u32>(0x7FC0_0000, 0, false, false, false),
            0x0000_0000
        );

        // SNAN -> 0
        assert_eq!(
            processor.fp_to_fixed::<u32, u32>(0x7F80_0001, 0, false, false, false),
            0x0000_0000
        );
    }

    #[test]
    fn test_fp_to_fixed_f32_u32() {
        let mut processor = Processor::new();

        // 1.0 -> 1 (unsigned)
        assert_eq!(
            processor.fp_to_fixed::<u32, u32>(0x3F80_0000, 0, true, false, false),
            0x0000_0001
        );

        // 42.0 -> 42 (unsigned)
        assert_eq!(
            processor.fp_to_fixed::<u32, u32>(0x4228_0000, 0, true, false, false),
            0x0000_002A
        );

        // positive infinity -> max value
        assert_eq!(
            processor.fp_to_fixed::<u32, u32>(0x7F80_0000, 0, true, false, false),
            0xFFFF_FFFF
        );

        // negative infinity -> 0
        assert_eq!(
            processor.fp_to_fixed::<u32, u32>(0xFF80_0000, 0, true, false, false),
            0x0000_0000
        );

        // -42.0 -> 0 (unsigned)
        assert_eq!(
            processor.fp_to_fixed::<u32, u32>(0xC228_0000, 0, true, false, false),
            0x0000_0000
        );

        // QNAN -> 0
        assert_eq!(
            processor.fp_to_fixed::<u32, u32>(0x7FC0_0000, 0, true, false, false),
            0x0000_0000
        );

        //SNAN -> 0
        assert_eq!(
            processor.fp_to_fixed::<u32, u32>(0x7F80_0001, 0, true, false, false),
            0x0000_0000
        );
    }

    #[test]
    fn test_fp_to_fixed_f64_s32() {
        let mut processor = Processor::new();

        // 1.0 -> 1 (signed)
        assert_eq!(
            processor.fp_to_fixed::<u64, u32>(0x3FF0_0000_0000_0000, 0, false, false, false),
            0x0000_0001
        );

        // 42.0 -> 42 (signed)
        assert_eq!(
            processor.fp_to_fixed::<u64, u32>(0x4045_0000_0000_0000, 0, false, false, false),
            0x0000_002A
        );

        // -1.0 -> -1 (signed)
        assert_eq!(
            processor.fp_to_fixed::<u64, u32>(0xBFF0_0000_0000_0000, 0, false, false, false),
            0xFFFF_FFFF
        );

        // -42.0 -> -42 (signed)
        assert_eq!(
            processor.fp_to_fixed::<u64, u32>(0xC045_0000_0000_0000, 0, false, false, false),
            0xFFFF_FFD6
        );

        // positive infinity -> max value
        assert_eq!(
            processor.fp_to_fixed::<u64, u32>(0x7FF0_0000_0000_0000, 0, false, false, false),
            0x7FFF_FFFF
        );

        // negative infinity -> min value
        assert_eq!(
            processor.fp_to_fixed::<u64, u32>(0xFFF0_0000_0000_0000, 0, false, false, false),
            0x8000_0000
        );

        // QNAN -> 0
        assert_eq!(
            processor.fp_to_fixed::<u64, u32>(0x7FF8_0000_0000_0000, 0, false, false, false),
            0x0000_0000
        );

        // SNAN -> 0
        assert_eq!(
            processor.fp_to_fixed::<u64, u32>(0x7FF0_0000_0000_0001, 0, false, false, false),
            0x0000_0000
        );
    }

    #[test]
    fn test_fp_to_fixed_f64_u32() {
        let mut processor = Processor::new();

        // 1.0 -> 1 (unsigned)
        assert_eq!(
            processor.fp_to_fixed::<u64, u32>(0x3FF0_0000_0000_0000, 0, true, false, false),
            0x0000_0001
        );

        // 42.0 -> 42 (unsigned)
        assert_eq!(
            processor.fp_to_fixed::<u64, u32>(0x4045_0000_0000_0000, 0, true, false, false),
            0x0000_002A
        );

        // positive infinity -> max value
        assert_eq!(
            processor.fp_to_fixed::<u64, u32>(0x7FF0_0000_0000_0000, 0, true, false, false),
            0xFFFF_FFFF
        );

        // negative infinity -> 0
        assert_eq!(
            processor.fp_to_fixed::<u64, u32>(0xFFF0_0000_0000_0000, 0, true, false, false),
            0x0000_0000
        );

        // -42.0 -> 0 (unsigned)
        assert_eq!(
            processor.fp_to_fixed::<u64, u32>(0xC045_0000_0000_0000, 0, true, false, false),
            0x0000_0000
        );

        // QNAN -> 0
        assert_eq!(
            processor.fp_to_fixed::<u64, u32>(0x7FF8_0000_0000_0000, 0, true, false, false),
            0x0000_0000
        );

        // SNAN -> 0
        assert_eq!(
            processor.fp_to_fixed::<u64, u32>(0x7FF0_0000_0000_0001, 0, true, false, false),
            0x0000_0000
        );
    }

    #[test]
    fn test_fixed_to_fp_s32_f32() {
        let mut processor = Processor::new();

        // 1 -> 1.0 (signed)
        assert_eq!(
            processor.fixed_to_fp::<u32, u32>(1, 0, false, false, false),
            0x3F80_0000
        );

        // 42 -> 42.0 (signed)
        assert_eq!(
            processor.fixed_to_fp::<u32, u32>(42, 0, false, false, false),
            0x4228_0000
        );

        // -1 -> -1.0 (signed)
        assert_eq!(
            processor.fixed_to_fp::<u32, u32>(0xffff_ffff, 0, false, false, false),
            0xBF80_0000
        );
    }

    #[test]
    fn test_preserve_fp_state_writes_registers_and_fpscr() {
        let mut processor = Processor::new();
        processor.fpcar = 0x2000_0100;
        processor.fpccr = 1 << FPCCR_LSPACT;
        processor.fpscr = 0xabcd_1234;

        for i in 0..16 {
            processor.set_sr(SingleReg::from(i as u8), 0x4444_0000 + i);
        }

        processor.preserve_fp_state().unwrap();

        for i in 0..16 {
            assert_eq!(
                processor.read32(0x2000_0100 + i * 4).unwrap(),
                0x4444_0000 + i
            );
        }
        assert_eq!(processor.read32(0x2000_0140).unwrap(), 0xabcd_1234);
        assert!(!processor.fpccr.get_bit(FPCCR_LSPACT));
    }

    #[test]
    fn test_preserve_fp_state_uses_unprivileged_access_type_when_user_bit_set() {
        let mut processor = Processor::new();
        processor.fpcar = 0x2000_0180;
        processor.fpccr = (1 << FPCCR_LSPACT) | (1 << FPCCR_USER);
        processor.set_sr(SingleReg::S0, 0xdead_beef);

        processor.preserve_fp_state().unwrap();

        assert_eq!(processor.read32(0x2000_0180).unwrap(), 0xdead_beef);
        assert!(!processor.fpccr.get_bit(FPCCR_LSPACT));
    }

    #[test]
    fn test_check_vfp_enabled_rejects_mismatched_coprocessor_access() {
        let mut processor = Processor::new();
        processor.cpacr = 0x0010_0000;

        assert_eq!(processor.check_vfp_enabled(), Err(Fault::Forced));
    }

    #[test]
    fn test_check_vfp_enabled_sets_nocp_when_vfp_disabled() {
        let mut processor = Processor::new();
        processor.cfsr = 0;

        assert_eq!(processor.check_vfp_enabled(), Err(Fault::Nocp));
        assert!(processor.cfsr.get_bit(19));
    }

    #[test]
    fn test_check_vfp_enabled_requires_privilege_for_privileged_only_access() {
        let mut processor = Processor::new();
        processor.cpacr = 0x0050_0000;
        processor.control.n_priv = true;

        assert_eq!(processor.check_vfp_enabled(), Err(Fault::Nocp));
        assert!(processor.cfsr.get_bit(19));
    }

    #[test]
    fn test_check_vfp_enabled_allows_privileged_only_access_in_privileged_mode() {
        let mut processor = Processor::new();
        processor.cpacr = 0x0050_0000;

        assert_eq!(processor.check_vfp_enabled(), Ok(()));
    }

    #[test]
    fn test_check_vfp_enabled_rejects_reserved_access_encoding() {
        let mut processor = Processor::new();
        processor.cpacr = 0x00a0_0000;

        assert_eq!(processor.check_vfp_enabled(), Err(Fault::Nocp));
        assert!(processor.cfsr.get_bit(19));
    }

    #[test]
    fn test_execute_fp_check_preserves_lazy_state_and_loads_fpdscr_control_bits() {
        let mut processor = Processor::new();
        processor.cpacr = 0x00f0_0000;
        processor.fpcar = 0x2000_0200;
        processor.fpccr = (1 << FPCCR_LSPACT) | (1 << FPCCR_ASPEN);
        processor.fpdscr = 0x07c0_0000;
        processor.fpscr = 0;
        processor.set_sr(SingleReg::S0, 0x1357_9bdf);

        processor.execute_fp_check().unwrap();

        assert_eq!(processor.read32(0x2000_0200).unwrap(), 0x1357_9bdf);
        assert_eq!(
            processor
                .fpscr
                .get_bits(FPSCR_STATUS_CONTROL_START..FPSCR_STATUS_CONTROL_END),
            processor
                .fpdscr
                .get_bits(FPSCR_STATUS_CONTROL_START..FPSCR_STATUS_CONTROL_END)
        );
        assert!(processor.control.fpca);
    }
}
