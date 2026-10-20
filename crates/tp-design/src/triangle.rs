use serde::Serialize;

/// Velocity triangle at one point of a blade row.
///
/// - `u`: blade (circumferential) speed;
/// - `cm`, `cu`: meridional and circumferential components of the absolute velocity c;
/// - the relative velocity is w = c − u, so its circumferential component is
///   w_u = u − c_u (positive against the direction of rotation, as usual for pumps).
///
/// Angles are measured from the circumferential direction, the pump convention of
/// Gülich (*Centrifugal Pumps*, 2nd ed., Tables 3.1 and 3.2) and Pfleiderer.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct VelocityTriangle {
    pub u: f64,
    pub cm: f64,
    pub cu: f64,
}

impl VelocityTriangle {
    pub fn new(u: f64, cm: f64, cu: f64) -> Self {
        Self { u, cm, cu }
    }

    /// Magnitude of the absolute velocity.
    pub fn c(&self) -> f64 {
        self.cm.hypot(self.cu)
    }

    /// Circumferential component of the relative velocity, u − c_u.
    pub fn wu(&self) -> f64 {
        self.u - self.cu
    }

    /// Magnitude of the relative velocity.
    pub fn w(&self) -> f64 {
        self.cm.hypot(self.wu())
    }

    /// Absolute flow angle α (rad) from the circumferential direction.
    pub fn alpha(&self) -> f64 {
        self.cm.atan2(self.cu)
    }

    /// Relative flow angle β (rad) from the circumferential direction.
    pub fn beta(&self) -> f64 {
        self.cm.atan2(self.wu())
    }
}

/// Specific work exchanged between rotor and fluid, Y_th = u₂·c_u2 − u₁·c_u1
/// (Euler's turbomachinery equation), in J/kg.
pub fn euler_work(inlet: &VelocityTriangle, outlet: &VelocityTriangle) -> f64 {
    outlet.u * outlet.cu - inlet.u * inlet.cu
}

/// Blockage factor τ = 1 / (1 − z·e / (π·d·sin β_B·sin λ)): the ratio between the
/// meridional velocity inside the blade channel and outside it, for `z` blades of
/// thickness `e` at diameter `d`, with blade angle β_B and blade inclination to the
/// meridional plane λ (90° for blades perpendicular to hub and shroud).
///
/// Returns `f64::INFINITY` when the blades would fill the whole circumference.
///
/// Gülich 2nd ed., eqs. 3.1.7 (inlet) and 3.2.3 (outlet).
pub fn blockage_factor(z: u32, e: f64, d: f64, blade_angle: f64, lambda: f64) -> f64 {
    if d <= 0.0 {
        return 1.0;
    }
    let blocked = f64::from(z) * e / (std::f64::consts::PI * d * blade_angle.sin() * lambda.sin());
    if blocked >= 1.0 {
        f64::INFINITY
    } else {
        1.0 / (1.0 - blocked)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use std::f64::consts::FRAC_PI_2;

    #[test]
    fn components_and_angles() {
        let t = VelocityTriangle::new(20.0, 3.0, 12.0);
        assert_relative_eq!(t.wu(), 8.0);
        assert_relative_eq!(t.w(), 73.0_f64.sqrt());
        assert_relative_eq!(t.c(), 153.0_f64.sqrt());
        assert_relative_eq!(t.beta().tan(), 3.0 / 8.0, epsilon = 1e-12);
        assert_relative_eq!(t.alpha().tan(), 3.0 / 12.0, epsilon = 1e-12);
    }

    #[test]
    fn swirl_free_inlet_has_axial_absolute_flow() {
        let inlet = VelocityTriangle::new(10.0, 4.0, 0.0);
        assert_relative_eq!(inlet.alpha(), FRAC_PI_2);
        let outlet = VelocityTriangle::new(25.0, 3.0, 13.0);
        assert_relative_eq!(euler_work(&inlet, &outlet), 325.0);
    }

    #[test]
    fn blockage() {
        // 6 blades of 3 mm at 172 mm, β_B = 25°: τ ≈ 1.085.
        let tau = blockage_factor(6, 0.003, 0.172, 25f64.to_radians(), FRAC_PI_2);
        assert_relative_eq!(
            tau,
            1.0 / (1.0 - 0.018 / (std::f64::consts::PI * 0.172 * 25f64.to_radians().sin()))
        );
        assert!(tau > 1.08 && tau < 1.09);
        assert!(blockage_factor(40, 0.02, 0.1, 0.1, FRAC_PI_2).is_infinite());
        assert_eq!(blockage_factor(6, 0.003, 0.0, 0.5, FRAC_PI_2), 1.0);
    }
}
