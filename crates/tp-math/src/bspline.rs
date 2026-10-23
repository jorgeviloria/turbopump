use serde::{Deserialize, Serialize};

use crate::linalg::Lu;
use crate::{Bezier, MathError, Point};

/// A non-rational B-spline curve.
///
/// Used for free-form hub and shroud contours and for fitting smooth curves through
/// computed points (blade sections, imported geometry).
///
/// Reference: L. Piegl, W. Tiller, *The NURBS Book*, 2nd ed., Springer 1997, ch. 2–3, 9.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "RawBSpline<P>",
    bound(deserialize = "P: Point + Deserialize<'de>")
)]
pub struct BSpline<P> {
    degree: usize,
    knots: Vec<f64>,
    control_points: Vec<P>,
}

#[derive(Deserialize)]
struct RawBSpline<P> {
    degree: usize,
    knots: Vec<f64>,
    control_points: Vec<P>,
}

impl<P: Point> TryFrom<RawBSpline<P>> for BSpline<P> {
    type Error = MathError;

    fn try_from(raw: RawBSpline<P>) -> Result<Self, Self::Error> {
        Self::new(raw.degree, raw.knots, raw.control_points)
    }
}

impl<P: Point> BSpline<P> {
    /// Creates a B-spline from explicit knots. Requires `knots.len() == control_points.len()
    /// + degree + 1`, non-decreasing finite knots and a non-empty parameter domain.
    pub fn new(degree: usize, knots: Vec<f64>, control_points: Vec<P>) -> Result<Self, MathError> {
        let n = control_points.len();
        if n < degree + 1 {
            return Err(MathError::TooFewPoints {
                needed: degree + 1,
                got: n,
            });
        }
        if knots.len() != n + degree + 1 {
            return Err(MathError::InvalidKnots(format!(
                "expected {} knots for {n} control points of degree {degree}, got {}",
                n + degree + 1,
                knots.len()
            )));
        }
        if knots.iter().any(|k| !k.is_finite()) || knots.windows(2).any(|w| w[1] < w[0]) {
            return Err(MathError::InvalidKnots(
                "knots must be finite and non-decreasing".into(),
            ));
        }
        if knots[degree] >= knots[n] {
            return Err(MathError::InvalidKnots("parameter domain is empty".into()));
        }
        Ok(Self {
            degree,
            knots,
            control_points,
        })
    }

    /// A clamped B-spline on [0, 1] with uniformly spaced interior knots. It starts at the
    /// first control point and ends at the last one.
    pub fn clamped_uniform(degree: usize, control_points: Vec<P>) -> Result<Self, MathError> {
        let n = control_points.len();
        if n < degree + 1 {
            return Err(MathError::TooFewPoints {
                needed: degree + 1,
                got: n,
            });
        }
        let spans = n - degree;
        let knots = std::iter::repeat_n(0.0, degree)
            .chain((0..=spans).map(|i| i as f64 / spans as f64))
            .chain(std::iter::repeat_n(1.0, degree))
            .collect();
        Self::new(degree, knots, control_points)
    }

    /// The clamped B-spline of `degree` that passes through every point in `points`
    /// (global interpolation with chord-length parameters and averaged knots,
    /// Piegl & Tiller §9.2.1).
    pub fn interpolate(points: &[P], degree: usize) -> Result<Self, MathError> {
        let n = points.len();
        if degree == 0 || n < degree + 1 {
            return Err(MathError::TooFewPoints {
                needed: degree.max(1) + 1,
                got: n,
            });
        }

        let chords: Vec<f64> = points.windows(2).map(|w| w[0].distance(w[1])).collect();
        if chords.iter().any(|&c| c <= 0.0) {
            return Err(MathError::CoincidentPoints);
        }
        let total: f64 = chords.iter().sum();
        let mut params = Vec::with_capacity(n);
        params.push(0.0);
        let mut acc = 0.0;
        for c in &chords {
            acc += c;
            params.push(acc / total);
        }
        params[n - 1] = 1.0;

        let mut knots = vec![0.0; degree + 1];
        for j in 1..n - degree {
            knots.push(params[j..j + degree].iter().sum::<f64>() / degree as f64);
        }
        knots.extend(std::iter::repeat_n(1.0, degree + 1));

        let mut matrix = vec![0.0; n * n];
        for (row, &u) in params.iter().enumerate() {
            let span = find_span(&knots, degree, n, u);
            let basis = basis_functions(&knots, degree, span, u);
            for (j, value) in basis.into_iter().enumerate() {
                matrix[row * n + span - degree + j] = value;
            }
        }
        let control_points = Lu::factor(matrix, n)?.solve(points);
        Self::new(degree, knots, control_points)
    }

    pub fn degree(&self) -> usize {
        self.degree
    }

    pub fn knots(&self) -> &[f64] {
        &self.knots
    }

    pub fn control_points(&self) -> &[P] {
        &self.control_points
    }

    /// Valid parameter range `(u_min, u_max)`.
    pub fn domain(&self) -> (f64, f64) {
        (
            self.knots[self.degree],
            self.knots[self.control_points.len()],
        )
    }

    /// Point at parameter `u`; values outside the domain are clamped to it.
    pub fn eval(&self, u: f64) -> P {
        let (u0, u1) = self.domain();
        let u = u.clamp(u0, u1);
        let n = self.control_points.len();
        let span = find_span(&self.knots, self.degree, n, u);
        basis_functions(&self.knots, self.degree, span, u)
            .into_iter()
            .enumerate()
            .fold(P::ZERO, |acc, (j, b)| {
                acc + self.control_points[span - self.degree + j] * b
            })
    }

    /// The derivative curve dC/du, one degree lower (Piegl & Tiller §3.3).
    pub fn derivative(&self) -> BSpline<P> {
        let p = self.degree;
        let n = self.control_points.len();
        if p == 0 {
            return BSpline {
                degree: 0,
                knots: self.knots.clone(),
                control_points: vec![P::ZERO; n],
            };
        }
        let control_points = (0..n - 1)
            .map(|i| {
                let span = self.knots[i + p + 1] - self.knots[i + 1];
                if span > 0.0 {
                    (self.control_points[i + 1] - self.control_points[i]) * (p as f64 / span)
                } else {
                    P::ZERO
                }
            })
            .collect();
        BSpline {
            degree: p - 1,
            knots: self.knots[1..self.knots.len() - 1].to_vec(),
            control_points,
        }
    }

    /// `count` points at equally spaced parameters over the domain, including both ends.
    pub fn sample(&self, count: usize) -> Vec<P> {
        let count = count.max(2);
        let (u0, u1) = self.domain();
        (0..count)
            .map(|i| self.eval(u0 + (u1 - u0) * i as f64 / (count - 1) as f64))
            .collect()
    }

    /// The same curve with knot `u` inserted once, one control point more (Boehm's
    /// algorithm, Piegl & Tiller Algorithm A5.1 with r = 1). Unchanged when `u` lies
    /// outside the open domain or is already a knot of multiplicity `degree`.
    pub fn insert_knot(&self, u: f64) -> BSpline<P> {
        let p = self.degree;
        let n = self.control_points.len();
        let (u0, u1) = self.domain();
        let multiplicity = multiplicity(&self.knots, u);
        if !(u > u0 && u < u1) || multiplicity >= p {
            return self.clone();
        }
        let k = find_span(&self.knots, p, n, u);
        let s = multiplicity;
        let old = &self.control_points;
        let control_points = (0..=n)
            .map(|i| {
                if i + p <= k {
                    old[i]
                } else if i + s <= k {
                    let alpha = (u - self.knots[i]) / (self.knots[i + p] - self.knots[i]);
                    old[i] * alpha + old[i - 1] * (1.0 - alpha)
                } else {
                    old[i - 1]
                }
            })
            .collect();
        let mut knots = self.knots.clone();
        knots.insert(k + 1, u);
        BSpline {
            degree: p,
            knots,
            control_points,
        }
    }

    /// A clamped curve as Bézier pieces of its degree, one per non-empty knot span in
    /// order: every interior knot is inserted up to multiplicity `degree` (Piegl &
    /// Tiller §5.4). Each piece covers its span with t ∈ [0, 1].
    pub fn to_beziers(&self) -> Vec<Bezier<P>> {
        let p = self.degree.max(1);
        let (u0, u1) = self.domain();
        let mut interior: Vec<f64> = self
            .knots
            .iter()
            .copied()
            .filter(|&k| k > u0 && k < u1)
            .collect();
        interior.dedup();
        let mut spline = self.clone();
        for u in interior {
            while multiplicity(&spline.knots, u) < p {
                spline = spline.insert_knot(u);
            }
        }
        spline
            .control_points
            .windows(p + 1)
            .step_by(p)
            .map(|w| Bezier::new(w.to_vec()).expect("pieces have degree + 1 points"))
            .collect()
    }
}

/// How many times `u` appears in `knots`; inserted knots repeat their value exactly, so
/// the margin only absorbs rounding in stored files.
pub fn multiplicity(knots: &[f64], u: f64) -> usize {
    knots.iter().filter(|k| (**k - u).abs() <= 1e-12).count()
}

/// Index of the knot span containing `u` (Piegl & Tiller, Algorithm A2.1).
fn find_span(knots: &[f64], degree: usize, n_control: usize, u: f64) -> usize {
    let last = n_control - 1;
    if u >= knots[last + 1] {
        // At the end of the domain, use the last non-empty span.
        return (degree..=last)
            .rev()
            .find(|&i| knots[i] < knots[i + 1])
            .unwrap_or(last);
    }
    if u <= knots[degree] {
        return (degree..=last)
            .find(|&i| knots[i] < knots[i + 1])
            .unwrap_or(degree);
    }
    let (mut low, mut high) = (degree, last + 1);
    let mut mid = (low + high) / 2;
    while u < knots[mid] || u >= knots[mid + 1] {
        if u < knots[mid] {
            high = mid;
        } else {
            low = mid;
        }
        mid = (low + high) / 2;
    }
    mid
}

/// The `degree + 1` non-zero basis functions on `span` at `u` (Algorithm A2.2).
fn basis_functions(knots: &[f64], degree: usize, span: usize, u: f64) -> Vec<f64> {
    let mut values = vec![0.0; degree + 1];
    let mut left = vec![0.0; degree + 1];
    let mut right = vec![0.0; degree + 1];
    values[0] = 1.0;
    for j in 1..=degree {
        left[j] = u - knots[span + 1 - j];
        right[j] = knots[span + j] - u;
        let mut saved = 0.0;
        for r in 0..j {
            let temp = values[r] / (right[r + 1] + left[j - r]);
            values[r] = saved + right[r + 1] * temp;
            saved = left[j - r] * temp;
        }
        values[j] = saved;
    }
    values
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Bezier;
    use approx::assert_relative_eq;
    use glam::DVec2;
    use proptest::prelude::*;

    fn points() -> Vec<DVec2> {
        vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(1.0, 2.0),
            DVec2::new(3.0, 3.0),
            DVec2::new(5.0, 1.0),
            DVec2::new(6.0, 4.0),
        ]
    }

    #[test]
    fn clamped_curve_hits_end_control_points() {
        let s = BSpline::clamped_uniform(3, points()).unwrap();
        assert_eq!(s.eval(0.0), points()[0]);
        assert!(s.eval(1.0).distance(points()[4]) < 1e-14);
        assert_eq!(s.knots().len(), 9);
    }

    #[test]
    fn single_span_spline_equals_bezier() {
        let pts = points()[..4].to_vec();
        let s = BSpline::clamped_uniform(3, pts.clone()).unwrap();
        let b = Bezier::new(pts).unwrap();
        for t in [0.0, 0.3, 0.6, 1.0] {
            assert!(s.eval(t).distance(b.eval(t)) < 1e-14);
        }
    }

    #[test]
    fn degree_one_is_a_polyline() {
        let pts = vec![0.0, 2.0, 1.0];
        let s = BSpline::clamped_uniform(1, pts).unwrap();
        assert_relative_eq!(s.eval(0.25), 1.0);
        assert_relative_eq!(s.eval(0.75), 1.5);
    }

    #[test]
    fn basis_is_partition_of_unity() {
        let s = BSpline::clamped_uniform(3, points()).unwrap();
        for u in [0.0, 0.1, 0.5, 0.77, 1.0] {
            let span = find_span(s.knots(), 3, 5, u);
            let sum: f64 = basis_functions(s.knots(), 3, span, u).iter().sum();
            assert_relative_eq!(sum, 1.0, epsilon = 1e-14);
        }
    }

    #[test]
    fn interpolation_passes_through_points() {
        let pts = points();
        let s = BSpline::interpolate(&pts, 3).unwrap();
        // Recover each point's parameter by chord length and check the fit.
        let chords: Vec<f64> = pts.windows(2).map(|w| w[0].distance(w[1])).collect();
        let total: f64 = chords.iter().sum();
        let mut u = 0.0;
        assert!(s.eval(u).distance(pts[0]) < 1e-12);
        for (k, c) in chords.iter().enumerate() {
            u += c / total;
            assert!(s.eval(u).distance(pts[k + 1]) < 1e-12, "point {}", k + 1);
        }
    }

    #[test]
    fn interpolation_rejects_duplicates() {
        let pts = vec![DVec2::ZERO, DVec2::ZERO, DVec2::ONE];
        assert_eq!(
            BSpline::interpolate(&pts, 2).unwrap_err(),
            MathError::CoincidentPoints
        );
    }

    #[test]
    fn derivative_matches_finite_difference() {
        let s = BSpline::interpolate(&points(), 3).unwrap();
        let d = s.derivative();
        let h = 1e-6;
        for u in [0.15, 0.4, 0.8] {
            let fd = (s.eval(u + h) - s.eval(u - h)) / (2.0 * h);
            assert!(d.eval(u).distance(fd) < 1e-5);
        }
    }

    #[test]
    fn rejects_invalid_knots() {
        let pts = vec![0.0, 1.0, 2.0];
        assert!(BSpline::new(2, vec![0.0, 0.0, 0.0, 1.0, 1.0], pts.clone()).is_err());
        assert!(BSpline::new(2, vec![0.0, 0.0, 1.0, 0.5, 1.0, 1.0], pts.clone()).is_err());
        assert!(BSpline::new(2, vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0], pts).is_err());
    }

    #[test]
    fn serde_validates_on_load() {
        let s = BSpline::clamped_uniform(2, vec![0.0, 1.0, 3.0]).unwrap();
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<BSpline<f64>>(&json).unwrap(), s);
        let broken = r#"{"degree":2,"knots":[0,1],"control_points":[0,1,3]}"#;
        assert!(serde_json::from_str::<BSpline<f64>>(broken).is_err());
    }

    #[test]
    fn inserting_a_knot_keeps_the_curve() {
        let s = BSpline::clamped_uniform(3, points()).unwrap();
        let more = s.insert_knot(0.3);
        assert_eq!(more.control_points().len(), 6);
        assert_eq!(more.knots().len(), 10);
        for k in 0..=20 {
            let u = k as f64 / 20.0;
            assert!(more.eval(u).distance(s.eval(u)) < 1e-12, "u = {u}");
        }
        // Outside the domain, or at full multiplicity, nothing changes.
        assert_eq!(s.insert_knot(1.0), s);
        let full = s.insert_knot(0.5).insert_knot(0.5);
        assert_eq!(full.insert_knot(0.5), full);
    }

    #[test]
    fn a_spline_splits_into_bezier_pieces_on_its_spans() {
        for degree in 2..=4 {
            let s = BSpline::clamped_uniform(degree, points())
                .unwrap()
                .insert_knot(0.37);
            let pieces = s.to_beziers();
            let mut spans: Vec<f64> = s.knots().to_vec();
            spans.dedup();
            assert_eq!(pieces.len(), spans.len() - 1, "degree {degree}");
            for (piece, w) in pieces.iter().zip(spans.windows(2)) {
                assert_eq!(piece.degree(), degree);
                for k in 0..=10 {
                    let t = k as f64 / 10.0;
                    let u = w[0] + (w[1] - w[0]) * t;
                    assert!(piece.eval(t).distance(s.eval(u)) < 1e-12, "degree {degree}");
                }
            }
        }
    }

    proptest! {
        #[test]
        fn clamped_spline_stays_in_control_hull(
            ys in prop::collection::vec(-5.0..5.0f64, 4..10),
            u in 0.0..1.0f64,
        ) {
            let s = BSpline::clamped_uniform(3, ys.clone()).unwrap();
            let min = ys.iter().cloned().fold(f64::INFINITY, f64::min);
            let max = ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let v = s.eval(u);
            prop_assert!(v >= min - 1e-12 && v <= max + 1e-12);
        }
    }
}
