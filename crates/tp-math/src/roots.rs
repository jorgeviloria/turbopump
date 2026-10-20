//! Scalar root finding.

use crate::MathError;

const MAX_ITERATIONS: usize = 200;

/// Finds a root of `f` in [a, b] with Brent's method.
///
/// `f(a)` and `f(b)` must have opposite signs (or one of them be zero). Converges to
/// within `tol` in x. Combines bisection, secant and inverse quadratic interpolation.
///
/// Reference: R. P. Brent, *Algorithms for Minimization without Derivatives*,
/// Prentice-Hall 1973, ch. 4.
#[allow(clippy::float_cmp)] // `a == c` checks whether `a` was copied from `c`, not closeness.
pub fn brent(f: impl Fn(f64) -> f64, a: f64, b: f64, tol: f64) -> Result<f64, MathError> {
    let (mut a, mut b) = (a, b);
    let mut fa = f(a);
    let mut fb = f(b);
    if fa == 0.0 {
        return Ok(a);
    }
    if fb == 0.0 {
        return Ok(b);
    }
    if fa.signum() == fb.signum() {
        return Err(MathError::NotBracketed { a, b });
    }

    let mut c = a;
    let mut fc = fa;
    let mut d = b - a;
    let mut e = d;
    for _ in 0..MAX_ITERATIONS {
        if fb.signum() == fc.signum() {
            c = a;
            fc = fa;
            d = b - a;
            e = d;
        }
        if fc.abs() < fb.abs() {
            a = b;
            b = c;
            c = a;
            fa = fb;
            fb = fc;
            fc = fa;
        }
        let tol1 = 2.0 * f64::EPSILON * b.abs() + 0.5 * tol;
        let xm = 0.5 * (c - b);
        if xm.abs() <= tol1 || fb == 0.0 {
            return Ok(b);
        }
        if e.abs() >= tol1 && fa.abs() > fb.abs() {
            let s = fb / fa;
            let (mut p, mut q) = if a == c {
                (2.0 * xm * s, 1.0 - s)
            } else {
                let q = fa / fc;
                let r = fb / fc;
                (
                    s * (2.0 * xm * q * (q - r) - (b - a) * (r - 1.0)),
                    (q - 1.0) * (r - 1.0) * (s - 1.0),
                )
            };
            if p > 0.0 {
                q = -q;
            }
            p = p.abs();
            let min1 = 3.0 * xm * q - (tol1 * q).abs();
            let min2 = (e * q).abs();
            if 2.0 * p < min1.min(min2) {
                e = d;
                d = p / q;
            } else {
                d = xm;
                e = d;
            }
        } else {
            d = xm;
            e = d;
        }
        a = b;
        fa = fb;
        b += if d.abs() > tol1 { d } else { tol1.copysign(xm) };
        fb = f(b);
    }
    Err(MathError::NoConvergence(MAX_ITERATIONS))
}

/// Newton-Raphson iteration from `x0`, stopping when the step is below `tol`.
pub fn newton(
    f: impl Fn(f64) -> f64,
    df: impl Fn(f64) -> f64,
    x0: f64,
    tol: f64,
) -> Result<f64, MathError> {
    let mut x = x0;
    for _ in 0..MAX_ITERATIONS {
        let slope = df(x);
        if slope == 0.0 || !slope.is_finite() {
            return Err(MathError::NoConvergence(0));
        }
        let step = f(x) / slope;
        x -= step;
        if !x.is_finite() {
            break;
        }
        if step.abs() <= tol {
            return Ok(x);
        }
    }
    Err(MathError::NoConvergence(MAX_ITERATIONS))
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn brent_solves_dottie_number() {
        let x = brent(|x| x.cos() - x, 0.0, 1.0, 1e-14).unwrap();
        assert_relative_eq!(x, 0.739_085_133_215_160_6, epsilon = 1e-13);
    }

    #[test]
    fn brent_handles_reversed_interval_and_exact_roots() {
        let x = brent(|x| x * x - 2.0, 2.0, 0.0, 1e-14).unwrap();
        assert_relative_eq!(x, std::f64::consts::SQRT_2, epsilon = 1e-13);
        assert_eq!(brent(|x| x - 1.0, 1.0, 3.0, 1e-12).unwrap(), 1.0);
    }

    #[test]
    fn brent_requires_bracket() {
        assert!(matches!(
            brent(|x| x * x + 1.0, -1.0, 1.0, 1e-12),
            Err(MathError::NotBracketed { .. })
        ));
    }

    #[test]
    fn newton_converges_quadratically() {
        let x = newton(|x| x * x - 2.0, |x| 2.0 * x, 1.0, 1e-15).unwrap();
        assert_relative_eq!(x, std::f64::consts::SQRT_2, epsilon = 1e-15);
    }

    #[test]
    fn newton_reports_flat_derivative() {
        assert!(newton(|x| x * x + 1.0, |x| 2.0 * x, 0.0, 1e-12).is_err());
    }
}
