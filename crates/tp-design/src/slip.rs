//! Slip: the deviation of the outlet flow from the blade direction.

/// Empirical factor f₁ of the Wiesner–Gülich slip factor for radial impellers
/// (Gülich 2nd ed., eq. 3.2.6).
pub const F1_RADIAL: f64 = 0.98;

/// Factor f₁ for semi-axial impellers, f₁ = 1.02 + 1.2·10⁻³·(nq − 50)
/// (Gülich 2nd ed., eq. 3.2.6).
pub fn f1_semi_axial(nq: f64) -> f64 {
    1.02 + 1.2e-3 * (nq - 50.0)
}

/// Limiting inlet-to-outlet diameter ratio ε_lim = exp(−8.16·sin β₂B / z) of Wiesner's
/// correlation. Above it, the short blade channel reduces the slip factor.
pub fn wiesner_limit_ratio(blade_angle_out: f64, z: u32) -> f64 {
    (-8.16 * blade_angle_out.sin() / f64::from(z)).exp()
}

/// Slip factor γ of a radial impeller after Wiesner, in the form used by Gülich:
///
/// γ = f₁ · (1 − √(sin β₂B) / z^0.7) · k_w,
///
/// with k_w = 1 for d₁m/d₂ ≤ ε_lim and k_w = 1 − ((d₁m/d₂ − ε_lim) / (1 − ε_lim))³
/// otherwise. The outlet circumferential velocity then follows from
/// c_u2 = u₂ · (γ − φ₂·τ₂ / tan β₂B), with φ₂ = c_m2 / u₂.
///
/// `blade_angle_out` is β₂B in radians from the circumferential direction; `f1` is
/// [`F1_RADIAL`] for radial impellers.
///
/// References: F. J. Wiesner, "A review of slip factors for centrifugal impellers",
/// *J. Eng. Power* 89 (1967) 558–572; J. F. Gülich, *Centrifugal Pumps*, 2nd ed.,
/// Springer 2010, Table 3.2, eqs. 3.2.4–3.2.7 (checked against that edition).
pub fn wiesner_gulich(blade_angle_out: f64, z: u32, d1m_over_d2: f64, f1: f64) -> f64 {
    let base = 1.0 - blade_angle_out.sin().sqrt() / f64::from(z).powf(0.7);
    let limit = wiesner_limit_ratio(blade_angle_out, z);
    let kw = if d1m_over_d2 <= limit {
        1.0
    } else {
        1.0 - ((d1m_over_d2 - limit) / (1.0 - limit)).powi(3)
    };
    f1 * base * kw
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn wiesner_reference_value() {
        // β₂B = 22.5°, z = 6: σ = 1 − √(sin 22.5°)/6^0.7 = 0.8235 (Wiesner, f₁ = 1).
        let beta = 22.5f64.to_radians();
        assert_relative_eq!(wiesner_gulich(beta, 6, 0.3, 1.0), 0.8235, epsilon = 1e-4);
        // ε_lim = exp(−8.16·sin 22.5°/6) = exp(−0.52045)
        assert_relative_eq!(wiesner_limit_ratio(beta, 6), 0.59425, epsilon = 1e-5);
    }

    #[test]
    fn semi_axial_factor() {
        assert_relative_eq!(f1_semi_axial(50.0), 1.02);
        assert_relative_eq!(f1_semi_axial(150.0), 1.14, epsilon = 1e-12);
    }

    #[test]
    fn more_blades_less_slip() {
        let beta = 25f64.to_radians();
        assert!(wiesner_gulich(beta, 7, 0.3, F1_RADIAL) > wiesner_gulich(beta, 5, 0.3, F1_RADIAL));
    }

    #[test]
    fn short_channels_reduce_slip_factor() {
        let beta = 25f64.to_radians();
        let limit = wiesner_limit_ratio(beta, 6);
        let long = wiesner_gulich(beta, 6, limit, 1.0);
        let short = wiesner_gulich(beta, 6, limit + 0.2, 1.0);
        assert!(short < long);
    }
}
