use std::f64::consts::PI;

use crate::STANDARD_GRAVITY;

/// Specific speed of a turbomachine stage, stored in its dimensionless form.
///
/// All definitions describe the same quantity in different unit conventions:
///
/// | Accessor | Definition | Units |
/// |---|---|---|
/// | [`omega_s`](Self::omega_s) | ω·√Q / Y^¾ | dimensionless (ISO 2548 "type number" K) |
/// | [`nq`](Self::nq) | n·√Q / H^¾ | n in rpm, Q in m³/s, H in m (European pump practice) |
/// | [`ns_us`](Self::ns_us) | n·√Q / H^¾ | n in rpm, Q in US gpm, H in ft |
/// | [`cordier_sigma`](Self::cordier_sigma) | φ^½ / ψ^¾ | dimensionless, with φ = Q/(π/4·D²·u) and ψ = 2Y/u² |
///
/// Y = g·H is the specific work. Q is the flow per impeller eye: divide by two for a
/// double-suction impeller, and use the head of one stage for a multistage pump.
///
/// References: J. F. Gülich, *Centrifugal Pumps*, Springer, ch. 3 (nq);
/// O. Cordier, "Ähnlichkeitsbedingungen für Strömungsmaschinen", *BWK* 5 (1953) (σ);
/// S. L. Dixon, C. A. Hall, *Fluid Mechanics and Thermodynamics of Turbomachinery*,
/// ch. 2 (dimensionless form).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpecificSpeed {
    omega_s: f64,
}

/// Ratio ns_US / nq (≈ 51.65).
fn ns_us_per_nq() -> f64 {
    let gpm = crate::convert(1.0, "m³/s", "gpm").expect("gpm is defined");
    let foot = 0.3048_f64;
    gpm.sqrt() * foot.powf(0.75)
}

/// Ratio nq / ω_s (≈ 52.92).
fn nq_per_omega_s() -> f64 {
    STANDARD_GRAVITY.powf(0.75) * 60.0 / (2.0 * PI)
}

/// Ratio ω_s / σ = √π · 2^¾ (≈ 2.981).
fn omega_s_per_sigma() -> f64 {
    PI.sqrt() * 2.0_f64.powf(0.75)
}

impl SpecificSpeed {
    /// From flow `q` (m³/s), specific work `y` (J/kg) and rotational speed `omega` (rad/s).
    pub fn from_specific_work(q: f64, y: f64, omega: f64) -> Self {
        Self {
            omega_s: omega * q.sqrt() / y.powf(0.75),
        }
    }

    /// From flow `q` (m³/s), head `h` (m) and rotational speed `omega` (rad/s).
    pub fn from_head(q: f64, h: f64, omega: f64) -> Self {
        Self::from_specific_work(q, STANDARD_GRAVITY * h, omega)
    }

    pub fn from_omega_s(omega_s: f64) -> Self {
        Self { omega_s }
    }

    pub fn from_nq(nq: f64) -> Self {
        Self::from_omega_s(nq / nq_per_omega_s())
    }

    pub fn from_ns_us(ns_us: f64) -> Self {
        Self::from_nq(ns_us / ns_us_per_nq())
    }

    pub fn from_cordier_sigma(sigma: f64) -> Self {
        Self::from_omega_s(sigma * omega_s_per_sigma())
    }

    /// Dimensionless specific speed ω_s, also the ISO 2548 type number K.
    pub fn omega_s(&self) -> f64 {
        self.omega_s
    }

    /// European specific speed nq (rpm, m³/s, m).
    pub fn nq(&self) -> f64 {
        self.omega_s * nq_per_omega_s()
    }

    /// US specific speed Ns (rpm, gpm, ft).
    pub fn ns_us(&self) -> f64 {
        self.nq() * ns_us_per_nq()
    }

    /// Cordier speed number σ.
    pub fn cordier_sigma(&self) -> f64 {
        self.omega_s / omega_s_per_sigma()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    const RPM: f64 = PI / 30.0;

    #[test]
    fn textbook_conversion_factors() {
        // Gülich: nq = 52.9 ω_s; US practice: Ns = 51.6 nq; Cordier: σ = nq / 157.8.
        assert_relative_eq!(nq_per_omega_s(), 52.92, epsilon = 0.01);
        assert_relative_eq!(ns_us_per_nq(), 51.65, epsilon = 0.01);
        assert_relative_eq!(nq_per_omega_s() * omega_s_per_sigma(), 157.8, epsilon = 0.1);
    }

    #[test]
    fn pump_example() {
        // Q = 100 m³/h, H = 32 m, n = 2900 rpm → nq = 2900·√(0.02778)/32^0.75 ≈ 35.92
        let s = SpecificSpeed::from_head(100.0 / 3600.0, 32.0, 2900.0 * RPM);
        assert_relative_eq!(s.nq(), 35.92, epsilon = 0.01);
        assert_relative_eq!(s.omega_s(), 35.92 / 52.92, epsilon = 1e-3);
    }

    #[test]
    fn head_and_specific_work_agree() {
        let a = SpecificSpeed::from_head(0.1, 50.0, 150.0);
        let b = SpecificSpeed::from_specific_work(0.1, 50.0 * STANDARD_GRAVITY, 150.0);
        assert_relative_eq!(a.omega_s(), b.omega_s());
    }

    #[test]
    fn conversions_round_trip() {
        let s = SpecificSpeed::from_nq(25.0);
        assert_relative_eq!(
            SpecificSpeed::from_ns_us(s.ns_us()).nq(),
            25.0,
            epsilon = 1e-12
        );
        assert_relative_eq!(
            SpecificSpeed::from_cordier_sigma(s.cordier_sigma()).nq(),
            25.0,
            epsilon = 1e-12
        );
        assert_relative_eq!(
            SpecificSpeed::from_omega_s(s.omega_s()).nq(),
            25.0,
            epsilon = 1e-12
        );
    }
}
