use crate::{MathError, Point};

/// An open polyline with precomputed cumulative arc length.
#[derive(Debug, Clone, PartialEq)]
pub struct Polyline<P> {
    points: Vec<P>,
    cumulative: Vec<f64>,
}

impl<P: Point> Polyline<P> {
    /// Requires at least two points.
    pub fn new(points: Vec<P>) -> Result<Self, MathError> {
        if points.len() < 2 {
            return Err(MathError::TooFewPoints {
                needed: 2,
                got: points.len(),
            });
        }
        let mut cumulative = Vec::with_capacity(points.len());
        cumulative.push(0.0);
        for w in points.windows(2) {
            let last = *cumulative.last().expect("starts non-empty");
            cumulative.push(last + w[0].distance(w[1]));
        }
        Ok(Self { points, cumulative })
    }

    pub fn points(&self) -> &[P] {
        &self.points
    }

    /// Arc length from the first point to each vertex.
    pub fn cumulative_lengths(&self) -> &[f64] {
        &self.cumulative
    }

    pub fn length(&self) -> f64 {
        *self.cumulative.last().expect("at least two points")
    }

    /// Point at arc length `s` from the start, clamped to the polyline.
    pub fn point_at_length(&self, s: f64) -> P {
        let s = s.clamp(0.0, self.length());
        let i = self
            .cumulative
            .partition_point(|&c| c <= s)
            .clamp(1, self.points.len() - 1);
        let (s0, s1) = (self.cumulative[i - 1], self.cumulative[i]);
        let t = if s1 > s0 { (s - s0) / (s1 - s0) } else { 0.0 };
        self.points[i - 1].lerp(self.points[i], t)
    }

    /// Point at fraction `f ∈ [0, 1]` of the total length.
    pub fn point_at_fraction(&self, f: f64) -> P {
        self.point_at_length(f * self.length())
    }

    /// `count` points equally spaced by arc length, including both ends.
    pub fn resample(&self, count: usize) -> Vec<P> {
        let count = count.max(2);
        (0..count)
            .map(|i| self.point_at_fraction(i as f64 / (count - 1) as f64))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use glam::DVec2;

    fn l_shape() -> Polyline<DVec2> {
        Polyline::new(vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(3.0, 0.0),
            DVec2::new(3.0, 1.0),
        ])
        .unwrap()
    }

    #[test]
    fn length_and_lookup() {
        let p = l_shape();
        assert_relative_eq!(p.length(), 4.0);
        assert_eq!(p.point_at_length(1.5), DVec2::new(1.5, 0.0));
        assert_eq!(p.point_at_length(3.5), DVec2::new(3.0, 0.5));
        assert_eq!(p.point_at_length(-1.0), DVec2::new(0.0, 0.0));
        assert_eq!(p.point_at_length(10.0), DVec2::new(3.0, 1.0));
    }

    #[test]
    fn resample_is_uniform_in_length() {
        let pts = l_shape().resample(5);
        assert_eq!(pts.len(), 5);
        assert_eq!(pts[2], DVec2::new(2.0, 0.0));
        assert_eq!(pts[4], DVec2::new(3.0, 1.0));
    }

    #[test]
    fn tolerates_repeated_vertices() {
        let p = Polyline::new(vec![0.0, 1.0, 1.0, 2.0]).unwrap();
        assert_relative_eq!(p.point_at_length(1.0), 1.0);
        assert_relative_eq!(p.point_at_length(1.5), 1.5);
    }

    #[test]
    fn needs_two_points() {
        assert!(Polyline::new(vec![1.0]).is_err());
    }
}
