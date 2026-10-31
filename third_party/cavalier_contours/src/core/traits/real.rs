use super::FuzzyOrd;
use static_aabb2d_index::IndexableNum;

/// Trait representing a real number (e.g. 1.1, -3.5, etc.) that can be fuzzy compared and ordered.
pub trait Real:
    num_traits::real::Real
    + num_traits::Bounded
    + FuzzyOrd
    + std::default::Default
    + std::fmt::Debug
    + IndexableNum
    + 'static
{
    #[inline]
    #[must_use]
    fn pi() -> Self {
        Self::from(std::f64::consts::PI).unwrap()
    }

    #[inline]
    #[must_use]
    fn tau() -> Self {
        Self::from(std::f64::consts::TAU).unwrap()
    }

    #[inline]
    #[must_use]
    fn two() -> Self {
        Self::one() + Self::one()
    }

    #[inline]
    #[must_use]
    fn four() -> Self {
        Self::two() + Self::two()
    }

    #[inline]
    #[must_use]
    fn min_value() -> Self {
        num_traits::real::Real::min_value()
    }

    #[inline]
    #[must_use]
    fn max_value() -> Self {
        num_traits::real::Real::max_value()
    }

    // FluutCAM: trigonometry through the pure-Rust `libm` crate instead of the
    // platform maths library, so results are bit-identical on every platform.

    /// `(sin, cos)` from `libm`.
    #[must_use]
    fn libm_sin_cos(self) -> (Self, Self);

    /// `tan` from `libm`.
    #[must_use]
    fn libm_tan(self) -> Self;

    /// `atan` from `libm`.
    #[must_use]
    fn libm_atan(self) -> Self;

    /// `atan2(self, x)` from `libm`.
    #[must_use]
    fn libm_atan2(self, x: Self) -> Self;

    /// `acos` from `libm`.
    #[must_use]
    fn libm_acos(self) -> Self;
}

impl Real for f32 {
    #[inline]
    fn pi() -> Self {
        std::f32::consts::PI
    }

    #[inline]
    fn tau() -> Self {
        std::f32::consts::TAU
    }

    #[inline]
    fn two() -> Self {
        2.0f32
    }

    #[inline]
    fn four() -> Self {
        4.0f32
    }

    #[inline]
    fn libm_sin_cos(self) -> (Self, Self) {
        libm::sincosf(self)
    }

    #[inline]
    fn libm_tan(self) -> Self {
        libm::tanf(self)
    }

    #[inline]
    fn libm_atan(self) -> Self {
        libm::atanf(self)
    }

    #[inline]
    fn libm_atan2(self, x: Self) -> Self {
        libm::atan2f(self, x)
    }

    #[inline]
    fn libm_acos(self) -> Self {
        libm::acosf(self)
    }
}

impl Real for f64 {
    #[inline]
    fn pi() -> Self {
        std::f64::consts::PI
    }

    #[inline]
    fn tau() -> Self {
        std::f64::consts::TAU
    }

    #[inline]
    fn two() -> Self {
        2.0f64
    }

    #[inline]
    fn four() -> Self {
        4.0f64
    }

    #[inline]
    fn libm_sin_cos(self) -> (Self, Self) {
        libm::sincos(self)
    }

    #[inline]
    fn libm_tan(self) -> Self {
        libm::tan(self)
    }

    #[inline]
    fn libm_atan(self) -> Self {
        libm::atan(self)
    }

    #[inline]
    fn libm_atan2(self, x: Self) -> Self {
        libm::atan2(self, x)
    }

    #[inline]
    fn libm_acos(self) -> Self {
        libm::acos(self)
    }
}
