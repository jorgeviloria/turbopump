//! Minimization without derivatives.

/// Minimizes `f` from `start` with the downhill simplex method of Nelder and Mead, the
/// first simplex spanning `steps` along each coordinate. Stops when the values at the
/// vertices of the simplex differ by less than `tol` or after `max_evals` evaluations,
/// and returns the best point and its value.
///
/// Reference: J. A. Nelder, R. Mead, "A simplex method for function minimization",
/// *The Computer Journal* 7 (1965) 308–313, with the usual coefficients: reflection 1,
/// expansion 2, contraction ½ and shrink ½.
pub fn nelder_mead(
    f: impl Fn(&[f64]) -> f64,
    start: &[f64],
    steps: &[f64],
    tol: f64,
    max_evals: usize,
) -> (Vec<f64>, f64) {
    let n = start.len();
    let mut simplex: Vec<Vec<f64>> = vec![start.to_vec()];
    for i in 0..n {
        let mut p = start.to_vec();
        p[i] += steps[i];
        simplex.push(p);
    }
    let mut values: Vec<f64> = simplex.iter().map(|p| f(p)).collect();
    let mut evals = n + 1;
    let at = |a: &[f64], b: &[f64], t: f64| -> Vec<f64> {
        a.iter().zip(b).map(|(x, y)| x + t * (y - x)).collect()
    };
    while evals < max_evals {
        let mut order: Vec<usize> = (0..=n).collect();
        order.sort_by(|&a, &b| values[a].total_cmp(&values[b]));
        simplex = order.iter().map(|&i| simplex[i].clone()).collect();
        values = order.iter().map(|&i| values[i]).collect();
        if (values[n] - values[0]).abs() <= tol {
            break;
        }
        // Centroid of every vertex but the worst.
        let centroid: Vec<f64> = (0..n)
            .map(|j| simplex[..n].iter().map(|p| p[j]).sum::<f64>() / n as f64)
            .collect();
        let reflected = at(&centroid, &simplex[n], -1.0);
        let fr = f(&reflected);
        evals += 1;
        if fr < values[0] {
            let expanded = at(&centroid, &simplex[n], -2.0);
            let fe = f(&expanded);
            evals += 1;
            (simplex[n], values[n]) = if fe < fr {
                (expanded, fe)
            } else {
                (reflected, fr)
            };
        } else if fr < values[n - 1] {
            (simplex[n], values[n]) = (reflected, fr);
        } else {
            // Contract toward the better of the worst vertex and its reflection.
            let (towards, f_towards) = if fr < values[n] {
                (reflected, fr)
            } else {
                (simplex[n].clone(), values[n])
            };
            let contracted = at(&centroid, &towards, 0.5);
            let fc = f(&contracted);
            evals += 1;
            if fc < f_towards {
                (simplex[n], values[n]) = (contracted, fc);
            } else {
                for i in 1..=n {
                    simplex[i] = at(&simplex[0], &simplex[i], 0.5);
                    values[i] = f(&simplex[i]);
                }
                evals += n;
            }
        }
    }
    let best = (0..=n)
        .min_by(|&a, &b| values[a].total_cmp(&values[b]))
        .expect("the simplex has vertices");
    (simplex[best].clone(), values[best])
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn finds_the_minimum_of_rosenbrock() {
        let rosenbrock = |x: &[f64]| (1.0 - x[0]).powi(2) + 100.0 * (x[1] - x[0] * x[0]).powi(2);
        let (x, value) = nelder_mead(rosenbrock, &[-1.2, 1.0], &[0.1, 0.1], 1e-14, 5000);
        assert_relative_eq!(x[0], 1.0, epsilon = 1e-4);
        assert_relative_eq!(x[1], 1.0, epsilon = 1e-4);
        assert!(value < 1e-8);
    }

    #[test]
    fn handles_a_kinked_objective() {
        let kink = |x: &[f64]| (x[0] - 0.3).abs().max((x[1] + 0.7).abs());
        let (x, _) = nelder_mead(kink, &[0.0, 0.0], &[0.2, 0.2], 1e-12, 2000);
        assert_relative_eq!(x[0], 0.3, epsilon = 1e-4);
        assert_relative_eq!(x[1], -0.7, epsilon = 1e-4);
    }
}
