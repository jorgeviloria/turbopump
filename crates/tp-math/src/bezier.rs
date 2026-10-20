use glam::DVec2;
use serde::{Deserialize, Serialize};

use crate::{MathError, Point, quad, roots};

/// Segments used for arc-length quadrature; 32 × 5 Gauss points is far below drawing
/// tolerance for the low-degree curves used in design diagrams.
const ARC_LENGTH_SEGMENTS: usize = 32;

/// A Bézier curve of any degree, parameterized on t ∈ [0, 1].
///
/// Design diagrams (meridional contours, angle and thickness distributions) are built
/// from these, with the control points edited directly by the user.
///
/// Reference: L. Piegl, W. Tiller, *The NURBS Book*, 2nd ed., Springer 1997, ch. 1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "RawBezier<P>",
    bound(deserialize = "P: Point + Deserialize<'de>")
)]
pub struct Bezier<P> {
    control_points: Vec<P>,
}

#[derive(Deserialize)]
struct RawBezier<P> {
    control_points: Vec<P>,
}

impl<P: Point> TryFrom<RawBezier<P>> for Bezier<P> {
    type Error = MathError;

    fn try_from(raw: RawBezier<P>) -> Result<Self, Self::Error> {
        Self::new(raw.control_points)
    }
}

impl<P: Point> Bezier<P> {
    /// Creates a curve of degree `control_points.len() - 1`. At least one point is needed;
    /// a single point is a constant (degree 0) curve.
    pub fn new(control_points: Vec<P>) -> Result<Self, MathError> {
        if control_points.is_empty() {
            return Err(MathError::TooFewPoints { needed: 1, got: 0 });
        }
        Ok(Self { control_points })
    }

    /// A straight segment from `a` to `b`.
    pub fn line(a: P, b: P) -> Self {
        Self {
            control_points: vec![a, b],
        }
    }

    pub fn degree(&self) -> usize {
        self.control_points.len() - 1
    }

    pub fn control_points(&self) -> &[P] {
        &self.control_points
    }

    /// Moves control point `index`, as when the user drags it in a diagram.
    pub fn set_control_point(&mut self, index: usize, point: P) {
        self.control_points[index] = point;
    }

    pub fn start(&self) -> P {
        self.control_points[0]
    }

    pub fn end(&self) -> P {
        self.control_points[self.degree()]
    }

    /// Point at parameter `t` (de Casteljau's algorithm).
    pub fn eval(&self, t: f64) -> P {
        let mut work = self.control_points.clone();
        let n = work.len();
        for level in 1..n {
            for i in 0..n - level {
                work[i] = work[i].lerp(work[i + 1], t);
            }
        }
        work[0]
    }

    /// The derivative curve dB/dt (the hodograph), one degree lower.
    pub fn derivative(&self) -> Bezier<P> {
        let n = self.degree();
        if n == 0 {
            return Bezier {
                control_points: vec![P::ZERO],
            };
        }
        let control_points = self
            .control_points
            .windows(2)
            .map(|w| (w[1] - w[0]) * n as f64)
            .collect();
        Bezier { control_points }
    }

    /// Splits the curve at `t` into two curves that together trace the original.
    pub fn split(&self, t: f64) -> (Bezier<P>, Bezier<P>) {
        let n = self.control_points.len();
        let mut work = self.control_points.clone();
        let mut left = Vec::with_capacity(n);
        let mut right = Vec::with_capacity(n);
        left.push(work[0]);
        right.push(work[n - 1]);
        for level in 1..n {
            for i in 0..n - level {
                work[i] = work[i].lerp(work[i + 1], t);
            }
            left.push(work[0]);
            right.push(work[n - 1 - level]);
        }
        right.reverse();
        (
            Bezier {
                control_points: left,
            },
            Bezier {
                control_points: right,
            },
        )
    }

    /// The same curve expressed with one more control point.
    pub fn elevate(&self) -> Bezier<P> {
        let p = &self.control_points;
        let n = self.degree();
        let mut q = Vec::with_capacity(n + 2);
        q.push(p[0]);
        for i in 1..=n {
            let a = i as f64 / (n + 1) as f64;
            q.push(p[i - 1] * a + p[i] * (1.0 - a));
        }
        q.push(p[n]);
        Bezier { control_points: q }
    }

    /// `count` points at equally spaced parameters, including both ends.
    pub fn sample(&self, count: usize) -> Vec<P> {
        let count = count.max(2);
        (0..count)
            .map(|i| self.eval(i as f64 / (count - 1) as f64))
            .collect()
    }

    /// Length of the curve between parameters `t0` and `t1`.
    pub fn arc_length_between(&self, t0: f64, t1: f64) -> f64 {
        let d = self.derivative();
        quad::gauss_legendre(|t| d.eval(t).norm(), t0, t1, ARC_LENGTH_SEGMENTS)
    }

    /// Total length of the curve.
    pub fn arc_length(&self) -> f64 {
        self.arc_length_between(0.0, 1.0)
    }
}

impl Bezier<DVec2> {
    /// Parameter where the curve has abscissa `x`, for curves used as functions y(x),
    /// whose x grows (or shrinks) monotonically with t.
    pub fn param_at_x(&self, x: f64) -> Result<f64, MathError> {
        let (x0, x1) = (self.start().x, self.end().x);
        let (min, max) = (x0.min(x1), x0.max(x1));
        if !(min..=max).contains(&x) {
            return Err(MathError::OutOfRange { value: x, min, max });
        }
        roots::brent(|t| self.eval(t).x - x, 0.0, 1.0, 1e-14)
    }

    /// Ordinate at abscissa `x`; see [`param_at_x`](Self::param_at_x).
    pub fn y_at_x(&self, x: f64) -> Result<f64, MathError> {
        Ok(self.eval(self.param_at_x(x)?).y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use proptest::prelude::*;

    fn cubic() -> Bezier<DVec2> {
        Bezier::new(vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(1.0, 2.0),
            DVec2::new(3.0, 3.0),
            DVec2::new(4.0, 0.0),
        ])
        .unwrap()
    }

    fn bernstein_cubic(p: &[DVec2], t: f64) -> DVec2 {
        let s = 1.0 - t;
        p[0] * s.powi(3) + p[1] * 3.0 * s * s * t + p[2] * 3.0 * s * t * t + p[3] * t.powi(3)
    }

    #[test]
    fn interpolates_end_points() {
        let c = cubic();
        assert_eq!(c.eval(0.0), c.start());
        assert_eq!(c.eval(1.0), c.end());
    }

    #[test]
    fn de_casteljau_matches_bernstein_form() {
        let c = cubic();
        for t in [0.1, 0.25, 0.5, 0.9] {
            let expected = bernstein_cubic(c.control_points(), t);
            assert_relative_eq!(c.eval(t).x, expected.x, epsilon = 1e-14);
            assert_relative_eq!(c.eval(t).y, expected.y, epsilon = 1e-14);
        }
    }

    #[test]
    fn derivative_matches_finite_difference() {
        let c = cubic();
        let d = c.derivative();
        let h = 1e-6;
        for t in [0.2, 0.5, 0.7] {
            let fd = (c.eval(t + h) - c.eval(t - h)) / (2.0 * h);
            assert_relative_eq!(d.eval(t).x, fd.x, epsilon = 1e-6);
            assert_relative_eq!(d.eval(t).y, fd.y, epsilon = 1e-6);
        }
        assert_eq!(Bezier::new(vec![3.0]).unwrap().derivative().eval(0.4), 0.0);
    }

    #[test]
    fn line_length_is_distance() {
        let line = Bezier::line(DVec2::new(1.0, 1.0), DVec2::new(4.0, 5.0));
        assert_relative_eq!(line.arc_length(), 5.0, epsilon = 1e-12);
    }

    #[test]
    fn quarter_circle_approximation_length() {
        // Standard cubic approximation of a unit quarter circle, k = 4/3·(√2 − 1).
        let k = 4.0 / 3.0 * (std::f64::consts::SQRT_2 - 1.0);
        let arc = Bezier::new(vec![
            DVec2::new(1.0, 0.0),
            DVec2::new(1.0, k),
            DVec2::new(k, 1.0),
            DVec2::new(0.0, 1.0),
        ])
        .unwrap();
        assert_relative_eq!(
            arc.arc_length(),
            std::f64::consts::FRAC_PI_2,
            epsilon = 1e-3
        );
    }

    #[test]
    fn y_at_x_on_monotone_curve() {
        let c = Bezier::new(vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(1.0, 1.0),
            DVec2::new(2.0, 2.0),
        ])
        .unwrap();
        assert_relative_eq!(c.y_at_x(1.3).unwrap(), 1.3, epsilon = 1e-12);
        assert!(matches!(c.y_at_x(2.5), Err(MathError::OutOfRange { .. })));
    }

    #[test]
    fn serde_round_trip_and_validation() {
        let c = cubic();
        let json = serde_json::to_string(&c).unwrap();
        assert_eq!(serde_json::from_str::<Bezier<DVec2>>(&json).unwrap(), c);
        assert!(serde_json::from_str::<Bezier<DVec2>>(r#"{"control_points":[]}"#).is_err());
    }

    fn arb_curve() -> impl Strategy<Value = Bezier<DVec2>> {
        prop::collection::vec((-10.0..10.0f64, -10.0..10.0f64), 2..7).prop_map(|pts| {
            Bezier::new(pts.into_iter().map(|(x, y)| DVec2::new(x, y)).collect()).unwrap()
        })
    }

    proptest! {
        #[test]
        fn split_halves_trace_the_curve(c in arb_curve(), t in 0.05..0.95f64, s in 0.0..1.0f64) {
            let (left, right) = c.split(t);
            let on_left = left.eval(s);
            let expected_left = c.eval(s * t);
            prop_assert!(on_left.distance(expected_left) < 1e-9);
            let on_right = right.eval(s);
            let expected_right = c.eval(t + s * (1.0 - t));
            prop_assert!(on_right.distance(expected_right) < 1e-9);
        }

        #[test]
        fn degree_elevation_keeps_shape(c in arb_curve(), t in 0.0..1.0f64) {
            let e = c.elevate();
            prop_assert_eq!(e.degree(), c.degree() + 1);
            prop_assert!(e.eval(t).distance(c.eval(t)) < 1e-9);
        }
    }
}
