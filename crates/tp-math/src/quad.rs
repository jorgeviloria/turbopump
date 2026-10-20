//! Numerical integration.

/// Nodes and weights of the 5-point Gauss-Legendre rule on [-1, 1]
/// (Abramowitz & Stegun, Table 25.4).
const GL5: [(f64, f64); 5] = [
    (0.0, 0.568_888_888_888_888_9),
    (-0.538_469_310_105_683_1, 0.478_628_670_499_366_5),
    (0.538_469_310_105_683_1, 0.478_628_670_499_366_5),
    (-0.906_179_845_938_664, 0.236_926_885_056_189_1),
    (0.906_179_845_938_664, 0.236_926_885_056_189_1),
];

/// Integrates `f` over [a, b] with the 5-point Gauss-Legendre rule on `segments` equal
/// sub-intervals. Exact for polynomials up to degree 9 on each segment.
pub fn gauss_legendre(f: impl Fn(f64) -> f64, a: f64, b: f64, segments: usize) -> f64 {
    let segments = segments.max(1);
    let h = (b - a) / segments as f64;
    (0..segments)
        .map(|s| {
            let mid = a + (s as f64 + 0.5) * h;
            GL5.iter()
                .map(|&(x, w)| w * f(mid + 0.5 * h * x))
                .sum::<f64>()
                * 0.5
                * h
        })
        .sum()
}

/// Integrates tabulated data with the trapezoidal rule. `xs` and `ys` must have the same
/// length; fewer than two samples give zero.
pub fn trapezoid(xs: &[f64], ys: &[f64]) -> f64 {
    debug_assert_eq!(xs.len(), ys.len());
    xs.windows(2)
        .zip(ys.windows(2))
        .map(|(x, y)| 0.5 * (x[1] - x[0]) * (y[0] + y[1]))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn exact_for_degree_nine_polynomials() {
        let f = |x: f64| x.powi(9) - 3.0 * x.powi(4) + 1.0;
        let exact = |x: f64| x.powi(10) / 10.0 - 3.0 * x.powi(5) / 5.0 + x;
        assert_relative_eq!(
            gauss_legendre(f, -0.5, 2.0, 1),
            exact(2.0) - exact(-0.5),
            epsilon = 1e-10
        );
    }

    #[test]
    fn integrates_smooth_functions() {
        assert_relative_eq!(
            gauss_legendre(f64::sin, 0.0, std::f64::consts::PI, 8),
            2.0,
            epsilon = 1e-12
        );
    }

    #[test]
    fn trapezoid_is_exact_for_lines() {
        let xs = [0.0, 0.5, 2.0, 3.0];
        let ys: Vec<f64> = xs.iter().map(|x| 2.0 * x + 1.0).collect();
        assert_relative_eq!(trapezoid(&xs, &ys), 12.0);
        assert_eq!(trapezoid(&[1.0], &[5.0]), 0.0);
    }
}
