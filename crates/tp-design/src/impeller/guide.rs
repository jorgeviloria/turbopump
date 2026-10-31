//! A meridional contour against Gülich's recommendations for the meridional section of
//! radial impellers (Centrifugal Pumps, 2nd ed., chap. 7.2.2.1, Fig. 7.4 and eq. 7.2).

use std::f64::consts::FRAC_PI_2;

use super::ImpellerBasis;
use super::meridian::MeridionalContour;

/// Reference specific speed of eq. 7.2.
const NQ_REF: f64 = 74.0;
/// Exponent of eq. 7.2.
const ZE_EXPONENT: f64 = 1.07;
/// Turn of the shroud from the axis that ends its nearly axial inlet run (our criterion).
const RUN_ANGLE: f64 = 10.0 * std::f64::consts::PI / 180.0;
/// Share of the recommended z_E within which the actual one counts as matching (ours:
/// Gülich allows shorter impellers when axial length matters).
const ZE_TOLERANCE: f64 = 0.25;

/// A value against its recommendation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Check {
    pub actual: f64,
    /// Recommended range; equal ends for a single value.
    pub recommended: (f64, f64),
    pub ok: bool,
}

impl Check {
    fn range(actual: f64, low: f64, high: f64) -> Self {
        Self {
            actual,
            recommended: (low, high),
            ok: (low..=high).contains(&actual),
        }
    }
}

/// The checks of a meridional contour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeridianGuide {
    /// Axial extension z_E from the inlet plane to the shroud end at the outlet, m,
    /// against eq. 7.2: z_E = (d₂ₐ − d₁)(n_q/74)^1.07.
    pub axial_extension: Check,
    /// Smallest radius of curvature of the shroud, m, against R_Ds = (0.6 to 0.8)·b₁,
    /// b₁ = (d₁ − d_n)/2.
    pub shroud_radius: Check,
    /// Nearly axial run of the shroud from the inlet, m, against g₁ = (0.2 to 0.3)·b₁.
    /// The run ends where the shroud has turned 10° from the axis (our criterion for
    /// Gülich's "only a minor increase in radius").
    pub inlet_run: Check,
    /// Whether the leading edge meets the shroud within that run: Gülich places it
    /// before the front shroud curves, not in its region of high curvature.
    pub leading_edge_before_bend: bool,
    /// Angle ε_Ds of the shroud at the outlet from the radial direction, rad. Gülich:
    /// often 0 below n_q = 20, rising to about 15–20° for radial impellers of higher n_q.
    pub shroud_outlet_angle: f64,
}

/// Checks `contour` against Gülich's recommendations.
pub fn guide(contour: &MeridionalContour, basis: &ImpellerBasis) -> MeridianGuide {
    let shroud = &contour.shroud;
    let b1 = (basis.inlet_diameter - basis.hub_diameter) / 2.0;

    let ze = (basis.outlet_diameter - basis.inlet_diameter) * (basis.nq / NQ_REF).powf(ZE_EXPONENT);
    // Axial positions from the inlet, at the shroud point of the inlet line.
    let z0 = shroud.start().x;
    let axial_extension = Check::range(
        shroud.end().x - z0,
        ze * (1.0 - ZE_TOLERANCE),
        ze * (1.0 + ZE_TOLERANCE),
    );
    let axial_extension = Check {
        recommended: (ze, ze),
        ..axial_extension
    };

    let kappa = shroud
        .curvature(80)
        .iter()
        .map(|&(_, k, _)| k)
        .fold(0.0_f64, f64::max);
    let radius = if kappa > 0.0 {
        1.0 / kappa
    } else {
        f64::INFINITY
    };
    let shroud_radius = Check::range(radius, 0.6 * b1, 0.8 * b1);

    let points = shroud.sample(600);
    let run = (points
        .windows(2)
        .find(|w| {
            let d = w[1] - w[0];
            d.y.atan2(d.x).abs() > RUN_ANGLE
        })
        .map_or(shroud.end().x, |w| w[0].x)
        - z0)
        .max(0.0);
    let inlet_run = Check::range(run, 0.2 * b1, 0.3 * b1);

    let leading_edge = contour.shroud_at(contour.leading_edge.1);
    let leading_edge_before_bend = leading_edge.x - z0 <= run + 1e-9;

    let out = shroud.end_direction();
    let shroud_outlet_angle = (FRAC_PI_2 - out.y.atan2(out.x)).abs();

    MeridianGuide {
        axial_extension,
        shroud_radius,
        inlet_run,
        leading_edge_before_bend,
        shroud_outlet_angle,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::impeller::meridian::MeridionalInput;
    use crate::impeller::meridian::tests::basis;
    use approx::assert_relative_eq;

    #[test]
    fn the_reference_impeller_against_eq_7_2() {
        let b = basis();
        let contour = MeridionalContour::build(&b, &MeridionalInput::default(), 1.0).unwrap();
        let g = guide(&contour, &b);
        // z_E = (172.2 − 72.3)·(35.92/74)^1.07 mm.
        let ze = (0.1722 - 0.0723) * (35.92_f64 / 74.0).powf(1.07);
        assert_relative_eq!(g.axial_extension.recommended.0, ze, max_relative = 1e-12);
        assert_relative_eq!(ze, 0.0461, max_relative = 2e-3);
        // The default contour reaches the shroud end at L − b₂, shorter than z_E.
        assert_relative_eq!(
            g.axial_extension.actual,
            contour.axial_length - b.outlet_width,
            max_relative = 1e-9
        );
        let b1 = (0.0723 - 0.0191) / 2.0;
        assert_relative_eq!(g.shroud_radius.recommended.0, 0.6 * b1);
        assert_relative_eq!(g.inlet_run.recommended.1, 0.3 * b1);
        // A radial impeller leaves radially.
        assert!(g.shroud_outlet_angle < 1e-6, "{}", g.shroud_outlet_angle);
        assert!(g.shroud_radius.actual.is_finite() && g.shroud_radius.actual > 0.0);
    }

    #[test]
    fn a_longer_impeller_moves_towards_the_recommended_extension() {
        let b = basis();
        let short = MeridionalContour::build(&b, &MeridionalInput::default(), 1.0).unwrap();
        let ze = guide(&short, &b).axial_extension.recommended.0;
        let input = MeridionalInput {
            axial_length: crate::Param::Manual(ze + b.outlet_width),
            ..MeridionalInput::default()
        };
        let long = MeridionalContour::build(&b, &input, 1.0).unwrap();
        let g = guide(&long, &b);
        assert_relative_eq!(g.axial_extension.actual, ze, max_relative = 1e-9);
        assert!(g.axial_extension.ok);
    }
}
