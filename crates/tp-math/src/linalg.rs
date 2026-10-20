//! Small dense linear systems.
//!
//! Enough for curve fitting, where systems have tens of unknowns. Large or sparse
//! problems (meridional flow, meshing) will use a dedicated library.

use crate::{MathError, Point};

/// LU factorization with partial pivoting of a square row-major matrix.
#[derive(Debug, Clone)]
pub struct Lu {
    n: usize,
    lu: Vec<f64>,
    perm: Vec<usize>,
}

impl Lu {
    /// Factors the `n × n` row-major matrix `a`.
    pub fn factor(mut a: Vec<f64>, n: usize) -> Result<Self, MathError> {
        assert_eq!(a.len(), n * n, "matrix must be n × n");
        let scale = a.iter().fold(0.0_f64, |m, v| m.max(v.abs()));
        let threshold = scale * f64::EPSILON * n as f64;
        let mut perm: Vec<usize> = (0..n).collect();

        for k in 0..n {
            let (pivot_row, pivot) = (k..n)
                .map(|i| (i, a[i * n + k].abs()))
                .max_by(|x, y| x.1.total_cmp(&y.1))
                .expect("k < n");
            if pivot <= threshold || pivot == 0.0 {
                return Err(MathError::Singular);
            }
            if pivot_row != k {
                for j in 0..n {
                    a.swap(pivot_row * n + j, k * n + j);
                }
                perm.swap(pivot_row, k);
            }
            for i in k + 1..n {
                let factor = a[i * n + k] / a[k * n + k];
                a[i * n + k] = factor;
                for j in k + 1..n {
                    a[i * n + j] -= factor * a[k * n + j];
                }
            }
        }
        Ok(Self { n, lu: a, perm })
    }

    /// Solves `A x = b`. The right-hand side may hold points, which solves one system
    /// per coordinate with a single factorization.
    pub fn solve<P: Point>(&self, b: &[P]) -> Vec<P> {
        let n = self.n;
        assert_eq!(b.len(), n, "right-hand side must have n entries");
        let mut x: Vec<P> = self.perm.iter().map(|&i| b[i]).collect();
        for i in 0..n {
            for j in 0..i {
                x[i] = x[i] - x[j] * self.lu[i * n + j];
            }
        }
        for i in (0..n).rev() {
            for j in i + 1..n {
                x[i] = x[i] - x[j] * self.lu[i * n + j];
            }
            x[i] = x[i] * (1.0 / self.lu[i * n + i]);
        }
        x
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use glam::DVec2;

    #[test]
    fn solves_system_needing_pivoting() {
        // 2y + z = 5 ; x + y + z = 6 ; 2x + y - z = 1  →  x = 1.8, y = 0.8, z = 3.4
        // The zero in the first pivot position forces a row swap.
        let a = vec![0.0, 2.0, 1.0, 1.0, 1.0, 1.0, 2.0, 1.0, -1.0];
        let lu = Lu::factor(a, 3).unwrap();
        let x = lu.solve(&[5.0, 6.0, 1.0]);
        assert_relative_eq!(x[0], 1.8, epsilon = 1e-12);
        assert_relative_eq!(x[1], 0.8, epsilon = 1e-12);
        assert_relative_eq!(x[2], 3.4, epsilon = 1e-12);
    }

    #[test]
    fn solves_point_right_hand_sides() {
        let lu = Lu::factor(vec![2.0, 0.0, 0.0, 4.0], 2).unwrap();
        let x = lu.solve(&[DVec2::new(2.0, 4.0), DVec2::new(8.0, 4.0)]);
        assert_eq!(x, vec![DVec2::new(1.0, 2.0), DVec2::new(2.0, 1.0)]);
    }

    #[test]
    fn detects_singular_matrix() {
        let a = vec![1.0, 2.0, 2.0, 4.0];
        assert_eq!(Lu::factor(a, 2).unwrap_err(), MathError::Singular);
    }
}
