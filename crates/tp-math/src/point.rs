use std::fmt::Debug;
use std::ops::{Add, Mul, Sub};

use glam::{DVec2, DVec3};

/// A value curves can interpolate: a point or vector in some space over `f64`.
pub trait Point:
    Copy + Debug + PartialEq + Add<Output = Self> + Sub<Output = Self> + Mul<f64, Output = Self>
{
    const ZERO: Self;

    /// Euclidean norm.
    fn norm(self) -> f64;

    /// Linear interpolation: `self` at `t = 0`, `other` at `t = 1`.
    fn lerp(self, other: Self, t: f64) -> Self {
        self + (other - self) * t
    }

    /// Distance between two points.
    fn distance(self, other: Self) -> f64 {
        (other - self).norm()
    }
}

impl Point for f64 {
    const ZERO: Self = 0.0;

    fn norm(self) -> f64 {
        self.abs()
    }
}

impl Point for DVec2 {
    const ZERO: Self = DVec2::ZERO;

    fn norm(self) -> f64 {
        self.length()
    }
}

impl Point for DVec3 {
    const ZERO: Self = DVec3::ZERO;

    fn norm(self) -> f64 {
        self.length()
    }
}
