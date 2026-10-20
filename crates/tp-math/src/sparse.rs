//! Sparse symmetric systems: assembly from triplets, compressed rows and the conjugate
//! gradient method with a diagonal (Jacobi) preconditioner.

use crate::MathError;

/// Collects (row, column, value) entries; repeated positions add up.
#[derive(Debug, Clone, Default)]
pub struct Triplets {
    size: usize,
    entries: Vec<(usize, usize, f64)>,
}

impl Triplets {
    /// An empty `size` × `size` matrix.
    pub fn new(size: usize) -> Self {
        Self {
            size,
            entries: Vec::new(),
        }
    }

    /// Adds `value` at (`row`, `col`).
    ///
    /// # Panics
    /// If the position lies outside the matrix.
    pub fn add(&mut self, row: usize, col: usize, value: f64) {
        assert!(
            row < self.size && col < self.size,
            "entry outside the matrix"
        );
        self.entries.push((row, col, value));
    }

    /// The matrix in compressed rows.
    pub fn build(mut self) -> SparseMatrix {
        self.entries.sort_by_key(|&(r, c, _)| (r, c));
        let mut row_start = vec![0; self.size + 1];
        let mut cols = Vec::with_capacity(self.entries.len());
        let mut values: Vec<f64> = Vec::with_capacity(self.entries.len());
        let mut last = None;
        for (r, c, v) in self.entries {
            if last == Some((r, c)) {
                *values.last_mut().expect("an entry was pushed") += v;
            } else {
                cols.push(c);
                values.push(v);
                row_start[r + 1] += 1;
                last = Some((r, c));
            }
        }
        for i in 0..self.size {
            row_start[i + 1] += row_start[i];
        }
        SparseMatrix {
            size: self.size,
            row_start,
            cols,
            values,
        }
    }
}

/// A square matrix in compressed sparse rows.
#[derive(Debug, Clone, PartialEq)]
pub struct SparseMatrix {
    size: usize,
    row_start: Vec<usize>,
    cols: Vec<usize>,
    values: Vec<f64>,
}

impl SparseMatrix {
    pub fn size(&self) -> usize {
        self.size
    }

    /// The stored entries of row `i` as (column, value).
    pub fn row(&self, i: usize) -> impl Iterator<Item = (usize, f64)> + '_ {
        let range = self.row_start[i]..self.row_start[i + 1];
        self.cols[range.clone()]
            .iter()
            .copied()
            .zip(self.values[range].iter().copied())
    }

    /// y = A·x.
    pub fn mul(&self, x: &[f64], y: &mut [f64]) {
        for (i, out) in y.iter_mut().enumerate().take(self.size) {
            *out = self.row(i).map(|(j, v)| v * x[j]).sum();
        }
    }

    fn diagonal(&self) -> Vec<f64> {
        (0..self.size)
            .map(|i| self.row(i).find(|&(j, _)| j == i).map_or(0.0, |(_, v)| v))
            .collect()
    }
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Solves A·x = b for a symmetric positive definite A by preconditioned conjugate
/// gradients, starting from the given `x`. Stops when ‖r‖ ≤ `tolerance`·‖b‖ and returns
/// the number of iterations.
pub fn conjugate_gradient(
    a: &SparseMatrix,
    b: &[f64],
    x: &mut [f64],
    tolerance: f64,
    max_iterations: usize,
) -> Result<usize, MathError> {
    let n = a.size;
    let inverse_diagonal: Vec<f64> = a
        .diagonal()
        .into_iter()
        .map(|d| if d.abs() > 0.0 { 1.0 / d } else { 1.0 })
        .collect();
    let mut ax = vec![0.0; n];
    a.mul(x, &mut ax);
    let mut r: Vec<f64> = b.iter().zip(&ax).map(|(b, ax)| b - ax).collect();
    let norm_b = dot(b, b).sqrt();
    let target = tolerance * if norm_b > 0.0 { norm_b } else { 1.0 };
    if dot(&r, &r).sqrt() <= target {
        return Ok(0);
    }
    let mut z: Vec<f64> = r
        .iter()
        .zip(&inverse_diagonal)
        .map(|(r, d)| r * d)
        .collect();
    let mut p = z.clone();
    let mut rz = dot(&r, &z);
    let mut ap = vec![0.0; n];
    for iteration in 1..=max_iterations {
        a.mul(&p, &mut ap);
        let pap = dot(&p, &ap);
        if pap <= 0.0 {
            return Err(MathError::Singular);
        }
        let alpha = rz / pap;
        for i in 0..n {
            x[i] += alpha * p[i];
            r[i] -= alpha * ap[i];
        }
        if dot(&r, &r).sqrt() <= target {
            return Ok(iteration);
        }
        for i in 0..n {
            z[i] = r[i] * inverse_diagonal[i];
        }
        let rz_next = dot(&r, &z);
        let beta = rz_next / rz;
        rz = rz_next;
        for i in 0..n {
            p[i] = z[i] + beta * p[i];
        }
    }
    Err(MathError::NoConvergence(max_iterations))
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    /// The 1D Laplacian with fixed ends: tridiagonal (−1, 2, −1).
    fn laplacian(n: usize) -> SparseMatrix {
        let mut t = Triplets::new(n);
        for i in 0..n {
            t.add(i, i, 2.0);
            if i > 0 {
                t.add(i, i - 1, -1.0);
            }
            if i + 1 < n {
                t.add(i, i + 1, -1.0);
            }
        }
        t.build()
    }

    #[test]
    fn repeated_entries_add_up() {
        let mut t = Triplets::new(2);
        t.add(0, 0, 1.0);
        t.add(1, 0, 3.0);
        t.add(0, 0, 2.0);
        let a = t.build();
        let row0: Vec<(usize, f64)> = a.row(0).collect();
        assert_eq!(row0, vec![(0, 3.0)]);
        let mut y = [0.0; 2];
        a.mul(&[1.0, 1.0], &mut y);
        assert_eq!(y, [3.0, 3.0]);
    }

    #[test]
    fn conjugate_gradient_solves_a_laplacian() {
        // −u'' = 0 with u(0) = 0, u(1) = 1 on n inner nodes: u_i = i/(n+1).
        let n = 50;
        let a = laplacian(n);
        let mut b = vec![0.0; n];
        b[n - 1] = 1.0;
        let mut x = vec![0.0; n];
        let iterations = conjugate_gradient(&a, &b, &mut x, 1e-12, 500).unwrap();
        assert!(iterations <= n + 1);
        for (i, v) in x.iter().enumerate() {
            assert_relative_eq!(*v, (i + 1) as f64 / (n + 1) as f64, epsilon = 1e-9);
        }
    }

    #[test]
    fn too_few_iterations_is_an_error() {
        let a = laplacian(50);
        let mut b = vec![0.0; 50];
        b[49] = 1.0;
        let mut x = vec![0.0; 50];
        assert_eq!(
            conjugate_gradient(&a, &b, &mut x, 1e-12, 3),
            Err(MathError::NoConvergence(3))
        );
    }
}
