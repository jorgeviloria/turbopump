//! Orientative membrane stresses in the casing wall of a volute, to choose its
//! thickness.
//!
//! Each cross section is taken as a ring of a thin-walled torus: a circle of the same
//! area, radius a = √(A/π), whose centre runs at radius R about an axis. For the spiral
//! the axis is the pump axis and R the radius of the section centroid; in the bend of a
//! radial discharge it is the axis of the bend, R = R_N; the straight part of the
//! diffuser is a straight pipe (R → ∞). Under internal pressure p, at the point of the
//! ring at angle θ with r = R + a·sin θ from the axis (our derivation, membrane theory):
//!
//! - equilibrium along the axis of the part of the torus between the crown (θ = 0) and
//!   that point, σ₁·t·2πr·sin θ = p·π·(r² − R²), gives the stress round the section
//!   σ₁ = p·a/t·(2R + a·sin θ)/(2·(R + a·sin θ)), largest on the inner side (sin θ = −1):
//!   σ₁ = p·a/t·(2R − a)/(2·(R − a));
//! - the membrane equation σ₁/a + σ₂·sin θ/r = p/t then gives the stress along the tube
//!   σ₂ = p·a/(2t), the same all round.
//!
//! For R → ∞ both are those of a straight pipe, p·a/t and p·a/(2t). Flat walls of
//! rectangular and trapezoidal sections also bend, which this estimate leaves out.
//!
//! The pressure is the rise over the suction pressure at shut-off, ρ·g·H₀, with
//! H₀ = H·ψ₀/ψ_opt from the shut-off coefficient of volute pumps (Gülich eq. 3.28) and
//! the optimum pressure coefficient (eq. 3.26). A pump whose suction is above the
//! atmosphere needs that pressure added.

use serde::Serialize;
use tp_units::STANDARD_GRAVITY as G;

use super::geometry::VoluteGeometry;
use crate::performance::{Collector, shutoff_coefficient};
use crate::pump::correlations::pressure_coefficient;
use crate::{DesignError, Param};

/// Default allowable stress, Pa: about a fifth of the tensile strength of a grey cast
/// iron of 250 MPa (our choice).
pub const DEFAULT_ALLOWABLE_STRESS: f64 = 50e6;

/// Where the wall is stressed most.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub enum StressPlace {
    /// A section of the spiral at this wrap angle from the cutwater, rad.
    Spiral { angle: f64 },
    /// The bend of a radial discharge.
    Bend,
    /// The straight part of the discharge diffuser.
    #[default]
    Straight,
}

/// The largest stresses in the casing wall.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct WallStress {
    /// Design pressure over the suction pressure and allowable stress, Pa.
    pub pressure: f64,
    pub allowable: f64,
    /// Wall thickness, m.
    pub thickness: f64,
    /// Largest stress round the section σ₁ and the stress along the tube σ₂ there, Pa.
    pub hoop: f64,
    pub longitudinal: f64,
    pub place: StressPlace,
    /// Radius of the equivalent circle there, m.
    pub section_radius: f64,
    /// Thickness at which σ₁ reaches the allowable stress, m.
    pub required_thickness: f64,
}

/// σ₁ on the inner side of a thin torus of section radius `a` whose centre runs at
/// radius `r` (m), wall `t` (m), pressure `p` (Pa); a straight pipe when `r` is infinite.
pub fn torus_hoop_stress(p: f64, a: f64, r: f64, t: f64) -> f64 {
    let factor = if r.is_finite() {
        (2.0 * r - a) / (2.0 * (r - a))
    } else {
        1.0
    };
    p * a / t * factor
}

/// σ₂ of a thin torus or pipe of section radius `a`, wall `t` and pressure `p`.
pub fn torus_longitudinal_stress(p: f64, a: f64, t: f64) -> f64 {
    p * a / (2.0 * t)
}

impl WallStress {
    /// The stresses in the casing of `g` at `pressure` (Pa, automatic: the shut-off
    /// pressure rise), compared with `allowable` (Pa, automatic: 50 MPa).
    pub fn new(
        g: &VoluteGeometry,
        pressure: &Param<f64>,
        allowable: &Param<f64>,
    ) -> Result<Self, DesignError> {
        let basis = &g.basis;
        let p = pressure.resolve(|| {
            let ratio =
                shutoff_coefficient(basis.nq, Collector::Volute) / pressure_coefficient(basis.nq);
            basis.density * G * basis.stage_head * ratio
        });
        let allowable = allowable.resolve(|| DEFAULT_ALLOWABLE_STRESS);
        if !(p > 0.0 && allowable > 0.0) {
            return Err(DesignError::InvalidInput(
                "the casing pressure and the allowable stress must be positive".into(),
            ));
        }
        let t = g.design.wall_thickness;
        let spiral = if g.design.rib.is_some() {
            &g.envelope
        } else {
            &g.sections
        };
        let mut candidates: Vec<(f64, f64, StressPlace)> = spiral
            .iter()
            .map(|s| {
                (
                    (s.size.area / std::f64::consts::PI).sqrt(),
                    s.size.centroid_radius,
                    StressPlace::Spiral { angle: s.angle },
                )
            })
            .collect();
        let bend = g.design.diffuser.bend_radius * g.nozzle_bend;
        let count = g.nozzle.len().saturating_sub(1).max(1) as f64;
        for (k, n) in g.nozzle.iter().enumerate().skip(1) {
            let along = g.nozzle_length * k as f64 / count;
            let a = (n.area / std::f64::consts::PI).sqrt();
            candidates.push(if along <= bend + 1e-9 {
                (a, g.design.diffuser.bend_radius, StressPlace::Bend)
            } else {
                (a, f64::INFINITY, StressPlace::Straight)
            });
        }
        let (a, r, place) = candidates
            .into_iter()
            .filter(|(a, r, _)| *a > 0.0 && *r > *a)
            .max_by(|x, y| {
                torus_hoop_stress(p, x.0, x.1, t).total_cmp(&torus_hoop_stress(p, y.0, y.1, t))
            })
            .ok_or_else(|| DesignError::Infeasible("the casing has no sections".into()))?;
        let hoop = torus_hoop_stress(p, a, r, t);
        Ok(Self {
            pressure: p,
            allowable,
            thickness: t,
            hoop,
            longitudinal: torus_longitudinal_stress(p, a, t),
            place,
            section_radius: a,
            // Thin-wall stresses fall as 1/t.
            required_thickness: t * hoop / allowable,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use std::f64::consts::PI;

    #[test]
    fn torus_stresses_balance_the_pressure() {
        let (p, a, r, t) = (4e5, 0.03, 0.12, 0.006);
        // Equilibrium of the part between the crown and the inner equator
        // (sin θ = −1, r_i = R − a): σ₁·t·2π·r_i = p·π·(R² − r_i²).
        let inner = r - a;
        assert_relative_eq!(
            torus_hoop_stress(p, a, r, t) * t * 2.0 * PI * inner,
            p * PI * (r * r - inner * inner),
            max_relative = 1e-12
        );
        // A far axis gives the straight pipe; the inner side carries more.
        assert_relative_eq!(
            torus_hoop_stress(p, a, f64::INFINITY, t),
            p * a / t,
            max_relative = 1e-12
        );
        assert!(torus_hoop_stress(p, a, r, t) > p * a / t);
        assert_relative_eq!(torus_longitudinal_stress(p, a, t), p * a / (2.0 * t));
    }

    #[test]
    fn the_reference_casing_is_far_below_cast_iron() {
        use crate::volute::VoluteInput;
        use crate::volute::tests::reference_basis;
        let basis = reference_basis();
        let g = VoluteGeometry::build(&basis, &VoluteInput::default()).unwrap();
        let w = g.wall_stress;
        // Shut-off rise over 32 m at n_q = 35.9: ψ₀/ψ_opt of eqs. 3.28 and 3.26.
        let ratio = 1.25 * (-0.3 * 0.359f64).exp() / (1.21 * (-0.77 * 0.359f64).exp());
        assert_relative_eq!(w.pressure, 998.2 * G * 32.0 * ratio, max_relative = 1e-9);
        // A few MPa in a 6 mm wall: a fraction of a millimetre would do.
        assert!(w.hoop > 1e6 && w.hoop < 10e6, "{}", w.hoop);
        assert!(w.required_thickness < 0.2 * w.thickness);
        assert_relative_eq!(
            w.longitudinal,
            w.pressure * w.section_radius / (2.0 * w.thickness)
        );
        assert!(g.warnings.iter().all(|m| !m.contains("allowable")));
        // A thin wall of a weak material draws a warning with the thickness it needs.
        let mut input = VoluteInput::default();
        input.wall.thickness = Param::Manual(0.002);
        input.wall.allowable_stress = Param::Manual(2e6);
        let thin = VoluteGeometry::build(&basis, &input).unwrap();
        assert!(thin.wall_stress.required_thickness > 0.002);
        assert!(thin.warnings.iter().any(|m| m.contains("allowable")));
    }
}
