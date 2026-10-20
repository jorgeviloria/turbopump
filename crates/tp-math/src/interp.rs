use serde::{Deserialize, Serialize};

use crate::MathError;

/// A 1D table y(x) with linear and monotone cubic interpolation.
///
/// This is the storage for empirical approximation functions (efficiency, head
/// coefficient, ... versus specific speed). Outside the table, values are held at the
/// end values: extrapolating published correlations is not reliable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RawTable", into = "RawTable")]
pub struct Table1D {
    xs: Vec<f64>,
    ys: Vec<f64>,
    slopes: Vec<f64>,
}

#[derive(Serialize, Deserialize)]
struct RawTable {
    x: Vec<f64>,
    y: Vec<f64>,
}

impl TryFrom<RawTable> for Table1D {
    type Error = MathError;

    fn try_from(raw: RawTable) -> Result<Self, Self::Error> {
        Self::new(raw.x, raw.y)
    }
}

impl From<Table1D> for RawTable {
    fn from(t: Table1D) -> Self {
        RawTable { x: t.xs, y: t.ys }
    }
}

impl Table1D {
    /// `xs` must be finite and strictly increasing, with as many entries as `ys` (≥ 2).
    pub fn new(xs: Vec<f64>, ys: Vec<f64>) -> Result<Self, MathError> {
        if xs.len() < 2 || ys.len() != xs.len() {
            return Err(MathError::TooFewPoints {
                needed: 2.max(xs.len()),
                got: xs.len().min(ys.len()),
            });
        }
        if xs.iter().chain(&ys).any(|v| !v.is_finite()) || xs.windows(2).any(|w| w[1] <= w[0]) {
            return Err(MathError::NotIncreasing);
        }
        let slopes = pchip_slopes(&xs, &ys);
        Ok(Self { xs, ys, slopes })
    }

    pub fn xs(&self) -> &[f64] {
        &self.xs
    }

    pub fn ys(&self) -> &[f64] {
        &self.ys
    }

    /// Index `i` of the interval [x_i, x_{i+1}] containing `x` (already clamped).
    fn interval(&self, x: f64) -> usize {
        self.xs
            .partition_point(|&xi| xi <= x)
            .clamp(1, self.xs.len() - 1)
            - 1
    }

    /// Piecewise linear interpolation.
    pub fn linear(&self, x: f64) -> f64 {
        let x = x.clamp(self.xs[0], *self.xs.last().expect("non-empty"));
        let i = self.interval(x);
        let t = (x - self.xs[i]) / (self.xs[i + 1] - self.xs[i]);
        self.ys[i] + t * (self.ys[i + 1] - self.ys[i])
    }

    /// Shape-preserving piecewise cubic (PCHIP): smooth like a spline, but never
    /// overshoots monotone data.
    pub fn pchip(&self, x: f64) -> f64 {
        let x = x.clamp(self.xs[0], *self.xs.last().expect("non-empty"));
        let i = self.interval(x);
        let h = self.xs[i + 1] - self.xs[i];
        let t = (x - self.xs[i]) / h;
        let (t2, t3) = (t * t, t * t * t);
        let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
        let h10 = t3 - 2.0 * t2 + t;
        let h01 = -2.0 * t3 + 3.0 * t2;
        let h11 = t3 - t2;
        h00 * self.ys[i]
            + h10 * h * self.slopes[i]
            + h01 * self.ys[i + 1]
            + h11 * h * self.slopes[i + 1]
    }
}

/// Node derivatives for monotone piecewise cubic Hermite interpolation.
///
/// Interior nodes use the weighted harmonic mean of F. N. Fritsch, J. Butland,
/// "A method for constructing local monotone piecewise cubic interpolants",
/// SIAM J. Sci. Stat. Comput. 5(2), 1984; end nodes use the shape-preserving
/// three-point formula of C. Moler, *Numerical Computing with MATLAB*, SIAM 2004, §3.4.
fn pchip_slopes(xs: &[f64], ys: &[f64]) -> Vec<f64> {
    let n = xs.len();
    let h: Vec<f64> = xs.windows(2).map(|w| w[1] - w[0]).collect();
    let delta: Vec<f64> = (0..n - 1).map(|i| (ys[i + 1] - ys[i]) / h[i]).collect();
    if n == 2 {
        return vec![delta[0]; 2];
    }

    let mut d = vec![0.0; n];
    for k in 1..n - 1 {
        if delta[k - 1] * delta[k] > 0.0 {
            let w1 = 2.0 * h[k] + h[k - 1];
            let w2 = h[k] + 2.0 * h[k - 1];
            d[k] = (w1 + w2) / (w1 / delta[k - 1] + w2 / delta[k]);
        }
    }
    d[0] = end_slope(h[0], h[1], delta[0], delta[1]);
    d[n - 1] = end_slope(h[n - 2], h[n - 3], delta[n - 2], delta[n - 3]);
    d
}

fn end_slope(h0: f64, h1: f64, del0: f64, del1: f64) -> f64 {
    let d = ((2.0 * h0 + h1) * del0 - h0 * del1) / (h0 + h1);
    if d.signum() != del0.signum() {
        0.0
    } else if del0.signum() != del1.signum() && d.abs() > (3.0 * del0).abs() {
        3.0 * del0
    } else {
        d
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use proptest::prelude::*;

    #[test]
    fn hits_nodes_exactly() {
        let t = Table1D::new(vec![0.0, 1.0, 3.0, 4.0], vec![1.0, 2.0, 0.0, 5.0]).unwrap();
        for (x, y) in t.xs().iter().zip(t.ys()) {
            assert_relative_eq!(t.linear(*x), *y, epsilon = 1e-14);
            assert_relative_eq!(t.pchip(*x), *y, epsilon = 1e-14);
        }
    }

    #[test]
    fn reproduces_linear_data() {
        let xs = vec![0.0, 0.5, 2.0, 3.5, 5.0];
        let ys: Vec<f64> = xs.iter().map(|x| 3.0 * x - 1.0).collect();
        let t = Table1D::new(xs, ys).unwrap();
        for x in [0.2, 1.1, 2.9, 4.4] {
            assert_relative_eq!(t.pchip(x), 3.0 * x - 1.0, epsilon = 1e-12);
            assert_relative_eq!(t.linear(x), 3.0 * x - 1.0, epsilon = 1e-12);
        }
    }

    #[test]
    fn clamps_outside_the_table() {
        let t = Table1D::new(vec![1.0, 2.0], vec![10.0, 20.0]).unwrap();
        assert_eq!(t.linear(0.0), 10.0);
        assert_eq!(t.pchip(5.0), 20.0);
    }

    #[test]
    fn rejects_bad_tables() {
        assert!(Table1D::new(vec![0.0], vec![1.0]).is_err());
        assert!(Table1D::new(vec![0.0, 1.0], vec![1.0]).is_err());
        assert!(Table1D::new(vec![0.0, 0.0], vec![1.0, 2.0]).is_err());
        assert!(Table1D::new(vec![0.0, f64::NAN], vec![1.0, 2.0]).is_err());
    }

    #[test]
    fn serializes_as_x_y_arrays() {
        let t = Table1D::new(vec![0.0, 1.0], vec![2.0, 3.0]).unwrap();
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(json, r#"{"x":[0.0,1.0],"y":[2.0,3.0]}"#);
        assert_eq!(serde_json::from_str::<Table1D>(&json).unwrap(), t);
    }

    proptest! {
        #[test]
        fn pchip_preserves_monotonicity(
            steps in prop::collection::vec((0.1..3.0f64, 0.0..5.0f64), 2..12),
            f in 0.0..1.0f64,
        ) {
            let mut xs = vec![0.0];
            let mut ys = vec![0.0];
            for (dx, dy) in &steps {
                xs.push(xs.last().unwrap() + dx);
                ys.push(ys.last().unwrap() + dy);
            }
            let t = Table1D::new(xs.clone(), ys).unwrap();
            let x_max = *xs.last().unwrap();
            let a = f * x_max;
            let b = (a + 0.01 * x_max).min(x_max);
            prop_assert!(t.pchip(b) >= t.pchip(a) - 1e-9);
        }
    }
}
