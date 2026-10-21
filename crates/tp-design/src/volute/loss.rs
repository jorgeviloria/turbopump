//! Hydraulic losses of a volute against flow.
//!
//! The spiral follows the one-dimensional model of D. Japikse, W. D. Marscher,
//! R. B. Furst, *Centrifugal Pump Design and Performance*, Concepts ETI 1997,
//! Chap. 4.3, eqs. 4.4 to 4.8. At the volute inlet (station 5) the swirl parameter is
//! λ = c_θ5/c_m5, and AR = A₇/A₅ is the throat area over the inlet area 2π·r₅·b₅:
//! - the meridional kinetic energy entering the spiral is lost:
//!   K_m = F₁/(1 + λ²) (eq. 4.7);
//! - a swirl that decelerates to the throat velocity c₇ is lost as in a sudden
//!   expansion: K_θ = F₂·(λ − 1/AR)²/(1 + λ²) when λ·AR > 1 (eq. 4.8), nothing when
//!   the flow accelerates;
//! - the static pressure recovery is C_p = 1 − 1/(AR²·(1 + λ²)) − K (eq. 4.6).
//!
//! The book lets the swirl diffuse as a free vortex from r₅ to a radius r₆ before the
//! expansion. We read that as the free-vortex swirl at the throat centroid,
//! λ₆ = λ·r₅/r₆, taking part in the expansion (our reading; the book writes the
//! factor (r₅/r₆)² on the loss of eq. 4.8 instead). With r₆ = r₅ both are eq. 4.8.
//! A spiral sized by angular momentum then has its least loss at its design flow, as
//! Gülich 2nd ed., Chap. 4.2, observes.
//!
//! The discharge cone after the throat loses (c_p,id − c_p)·c₇²/2g, with the ideal
//! recovery c_p,id = 1 − 1/A_R² (Gülich eq. 1.40) and the real one limited by the
//! best recovery a diffuser of that length reaches, c_p,opt = 0.36·(L/R₁)^0.26
//! (Gülich eq. 7.31).

use std::f64::consts::PI;

use tp_units::STANDARD_GRAVITY as G;

/// What the loss model needs from a volute, in SI units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VoluteLossGeometry {
    /// Inlet radius r₅, where the spiral starts collecting the flow, m.
    pub inlet_radius: f64,
    /// Inlet width b₅, m.
    pub inlet_width: f64,
    /// Throat area A₇, m².
    pub throat_area: f64,
    /// Radius of the throat centroid r₆, m.
    pub throat_radius: f64,
    /// Static pressure recovery of the discharge cone, ideal and estimated.
    pub cone_ideal_recovery: f64,
    pub cone_recovery: f64,
    /// Fraction F₁ of the meridional kinetic energy lost (1: all of it).
    pub meridional_factor: f64,
    /// Factor F₂ on the sudden-expansion loss of the swirl.
    pub swirl_factor: f64,
}

impl VoluteLossGeometry {
    /// The loss model of a designed volute: the spiral starts at the cutwater circle
    /// with the inlet width b₃, and the cone recovery comes from [`cone_recovery`]. A
    /// double volute counts both partial throats: each collects its share of the flow
    /// over its share of the inlet, so their sum gives the same AR.
    pub fn from_design(volute: &super::VoluteDesign) -> Self {
        let cone = &volute.diffuser;
        let throat_radius = (cone.inlet_area / PI).sqrt();
        let throat_area = volute.throat.area + volute.rib.map_or(0.0, |r| r.outer_throat.area);
        Self {
            inlet_radius: volute.cutwater_diameter / 2.0,
            inlet_width: volute.inlet_width,
            throat_area,
            throat_radius: volute.throat.centroid_radius,
            cone_ideal_recovery: cone.ideal_pressure_recovery,
            cone_recovery: cone_recovery(cone.area_ratio, cone.length / throat_radius),
            meridional_factor: 1.0,
            swirl_factor: 1.0,
        }
    }
}

/// Losses of the volute at one flow.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct VoluteLoss {
    /// Swirl parameter λ = c_θ5/c_m5 at the inlet.
    pub swirl: f64,
    /// AR = A₇/A₅.
    pub area_ratio: f64,
    /// λ₆·AR: below 1 the flow accelerates to the throat, above it decelerates.
    pub matching: f64,
    /// Loss coefficient of the spiral K = K_m + K_θ, on the inlet dynamic head.
    pub loss_coefficient: f64,
    /// Static pressure recovery of the spiral C_p.
    pub recovery: f64,
    /// Inlet velocity c₅, m/s.
    pub inlet_velocity: f64,
    /// Throat velocity c₇, m/s.
    pub throat_velocity: f64,
    /// Head lost in the spiral and in the discharge cone, m.
    pub spiral_loss: f64,
    pub cone_loss: f64,
}

impl VoluteLoss {
    /// Head lost in the whole volute, m.
    pub fn total(&self) -> f64 {
        self.spiral_loss + self.cone_loss
    }
}

/// Estimated static pressure recovery of a straight conical diffuser of area ratio
/// `area_ratio` and length `length_ratio` = L/R₁: the ideal one, limited by
/// c_p,opt = 0.36·(L/R₁)^0.26 (Gülich eq. 7.31).
pub fn cone_recovery(area_ratio: f64, length_ratio: f64) -> f64 {
    let ideal = 1.0 - 1.0 / (area_ratio * area_ratio);
    ideal.min(0.36 * length_ratio.max(0.0).powf(0.26)).max(0.0)
}

/// The losses of the volute for flow `flow` (m³/s) arriving with swirl `swirl`
/// c_θ5 (m/s) at its inlet radius.
pub fn volute_loss(geometry: &VoluteLossGeometry, flow: f64, swirl: f64) -> VoluteLoss {
    let inlet_area = 2.0 * PI * geometry.inlet_radius * geometry.inlet_width;
    let area_ratio = geometry.throat_area / inlet_area;
    let cm5 = flow / inlet_area;
    let c5_squared = cm5 * cm5 + swirl * swirl;
    let throat_velocity = flow / geometry.throat_area;
    // λ grows without bound as the flow vanishes; the coefficients stay finite, so
    // they are written on c_θ5 and c_m5 directly.
    let lambda = if cm5 > 0.0 {
        swirl / cm5
    } else {
        f64::INFINITY
    };
    let swirl_at_throat = swirl * geometry.inlet_radius / geometry.throat_radius;
    let meridional = geometry.meridional_factor * cm5 * cm5 / c5_squared;
    let expansion = (swirl_at_throat - throat_velocity).max(0.0);
    let tangential = geometry.swirl_factor * expansion * expansion / c5_squared;
    let loss_coefficient = meridional + tangential;
    let recovery = 1.0 - throat_velocity * throat_velocity / c5_squared - loss_coefficient;
    let cone_loss = (geometry.cone_ideal_recovery - geometry.cone_recovery).max(0.0)
        * throat_velocity
        * throat_velocity
        / (2.0 * G);
    VoluteLoss {
        swirl: lambda,
        area_ratio,
        matching: swirl_at_throat / throat_velocity.max(f64::MIN_POSITIVE),
        loss_coefficient,
        recovery,
        inlet_velocity: c5_squared.sqrt(),
        throat_velocity,
        spiral_loss: loss_coefficient * c5_squared / (2.0 * G),
        cone_loss,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    const FT: f64 = 0.3048;
    const INCH: f64 = 0.0254;

    /// The volute of the design example of Japikse et al. (Chap. 7.5, Fig. 7.73): a
    /// circular throat of 8.4804 in after a vaneless diffuser ending at r₅ = 12 in,
    /// b₅ = 3.1304 in, where c_m5 = 6.84 ft/s and c_θ5 = 28.58 ft/s. The conical
    /// diffuser to the 10.450 in flange recovers C_p = 0.377.
    fn book_example(throat_radius: f64) -> (VoluteLossGeometry, f64, f64) {
        let r5 = 12.0 * INCH;
        let b5 = 3.1304 * INCH;
        let d7 = 8.4804 * INCH;
        let a7 = PI / 4.0 * d7 * d7;
        let cone_area_ratio = (10.450 / 8.4804_f64).powi(2);
        let geometry = VoluteLossGeometry {
            inlet_radius: r5,
            inlet_width: b5,
            throat_area: a7,
            throat_radius,
            cone_ideal_recovery: 1.0 - 1.0 / (cone_area_ratio * cone_area_ratio),
            cone_recovery: 0.377,
            meridional_factor: 1.0,
            swirl_factor: 1.0,
        };
        let flow = 6.84 * FT * 2.0 * PI * r5 * b5;
        (geometry, flow, 28.58 * FT)
    }

    #[test]
    fn reproduces_the_volute_of_the_book_example() {
        let (geometry, flow, swirl) = book_example(12.0 * INCH);
        let loss = volute_loss(&geometry, flow, swirl);
        // The book prints AR = 0.24, λ·AR = 1.0, a spiral loss LC57 = 0.054 and a
        // recovery CP57 = 0.000.
        assert_relative_eq!(loss.area_ratio, 0.24, epsilon = 0.005);
        assert_relative_eq!(loss.matching, 1.0, epsilon = 0.005);
        assert_relative_eq!(loss.loss_coefficient, 0.054, epsilon = 0.0005);
        assert_relative_eq!(loss.recovery, 0.0, epsilon = 0.001);
        // Spiral and cone together, on the inlet dynamic head: LC58 = 0.234 and
        // CP58 = 0.356.
        let dynamic = loss.inlet_velocity.powi(2) / (2.0 * G);
        let overall = loss.total() / dynamic;
        assert_relative_eq!(overall, 0.234, epsilon = 0.001);
        let cone_area_ratio = (10.450 / 8.4804_f64).powi(2);
        let exit = loss.throat_velocity / cone_area_ratio;
        let recovery = 1.0 - (exit / loss.inlet_velocity).powi(2) - overall;
        assert_relative_eq!(recovery, 0.356, epsilon = 0.001);
    }

    #[test]
    fn a_free_vortex_before_the_throat_leaves_the_matched_example_unchanged() {
        // At λ·AR = 1 the spiral already accelerates once the swirl diffuses as a free
        // vortex, so only the meridional energy is lost.
        let (geometry, flow, swirl) = book_example(16.0 * INCH);
        let loss = volute_loss(&geometry, flow, swirl);
        assert_relative_eq!(loss.loss_coefficient, 0.054, epsilon = 0.0005);
        assert!(loss.matching < 1.0);
    }

    #[test]
    fn decelerating_flow_adds_the_sudden_expansion() {
        let (geometry, flow, swirl) = book_example(12.0 * INCH);
        let matched = volute_loss(&geometry, flow, swirl);
        // Half the flow with the same swirl: λ doubles and the spiral decelerates.
        let part = volute_loss(&geometry, 0.5 * flow, swirl);
        assert!(part.matching > 1.0);
        let lambda = part.swirl;
        let ar = part.area_ratio;
        let expected = (1.0 + (lambda - 1.0 / ar).powi(2)) / (1.0 + lambda * lambda);
        assert_relative_eq!(part.loss_coefficient, expected, epsilon = 1e-12);
        assert!(part.spiral_loss > matched.spiral_loss);
        // Diffusing flow recovers pressure: eq. 4.8 gives 2(λ − 1/AR)/(AR(1 + λ²)).
        let recovery = 2.0 * (lambda - 1.0 / ar) / (ar * (1.0 + lambda * lambda));
        assert_relative_eq!(part.recovery, recovery, epsilon = 1e-12);
    }

    #[test]
    fn the_cone_recovery_is_limited_by_its_length() {
        assert_relative_eq!(cone_recovery(1.0, 4.0), 0.0);
        // A short cone cannot reach its ideal recovery.
        let ideal = 1.0 - 1.0 / (2.23f64 * 2.23);
        assert!(cone_recovery(2.23, 4.0) < ideal);
        assert_relative_eq!(cone_recovery(2.23, 4.0), 0.36 * 4f64.powf(0.26));
        // A long, gentle one can.
        assert_relative_eq!(cone_recovery(1.2, 20.0), 1.0 - 1.0 / 1.44);
    }
}
