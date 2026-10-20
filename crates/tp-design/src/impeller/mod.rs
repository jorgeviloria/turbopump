//! Geometry of radial impellers: meridional contour, meridional flow and blades.
//!
//! The geometry steps start from the main dimensions, gathered in [`ImpellerBasis`],
//! so the same code serves any machine type that produces them.

pub mod blade;
pub mod meridian;

use std::f64::consts::PI;

use serde::{Deserialize, Serialize};

use crate::DesignError;
use crate::pump::correlations::{CorrelationInfo, Verification};
use crate::pump::{CentrifugalPumpDesign, CentrifugalPumpInput};
use blade::{Blade, BladePropertiesInput, DEFAULT_SPANS, EdgeInput, MeanLineInput, ProfileInput};
use meridian::{Channel, MeridionalContour, MeridionalInput};

/// Quasi-orthogonals used for the meridional flow.
const QUASI_ORTHOGONALS: usize = 81;

/// Main dimensions the geometry is built on, for the modeled side of the impeller
/// (for a double-entry impeller, one half: half the flow and half the outlet width).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImpellerBasis {
    /// Hub diameter at the eye, m.
    pub hub_diameter: f64,
    /// Shroud diameter at the eye, d₁, m.
    pub inlet_diameter: f64,
    /// d₂, m.
    pub outlet_diameter: f64,
    /// b₂ of the modeled side, m.
    pub outlet_width: f64,
    /// Flow through the modeled side including leakage, m³/s.
    pub flow: f64,
    /// rad/s
    pub omega: f64,
    pub blade_count: u32,
    /// Normal blade thickness, m.
    pub blade_thickness: f64,
    /// β₂B, rad.
    pub outlet_blade_angle: f64,
    /// Leading-edge incidence, rad.
    pub incidence: f64,
    /// +1 for positive rotation about +z, −1 for negative.
    pub rotation_sign: f64,
    /// Pressure coefficient ψ = 2gH/u₂² of the stage.
    pub pressure_coefficient: f64,
    pub hydraulic_efficiency: f64,
    /// Specific speed per stage and eye.
    pub nq: f64,
    /// Relative velocity at the impeller outlet w₂, m/s.
    pub outlet_relative_velocity: f64,
}

impl ImpellerBasis {
    /// The basis of a radial pump impeller from its main-dimensions step.
    pub fn from_pump(
        design: &CentrifugalPumpDesign,
        input: &CentrifugalPumpInput,
        rotation_sign: f64,
    ) -> Self {
        let eyes = input.suction.eyes();
        Self {
            hub_diameter: design.hub_diameter,
            inlet_diameter: design.inlet_diameter,
            outlet_diameter: design.outlet_diameter,
            outlet_width: design.outlet_width / eyes,
            flow: design.impeller_flow / eyes,
            omega: 2.0 * design.outlet.triangle.u / design.outlet_diameter,
            blade_count: design.blade_count,
            blade_thickness: design.blade_thickness,
            outlet_blade_angle: design.outlet.blade_angle,
            incidence: input.incidence.resolve(|| 0.0),
            rotation_sign,
            pressure_coefficient: design.pressure_coefficient,
            hydraulic_efficiency: design.efficiencies.hydraulic,
            nq: design.nq,
            outlet_relative_velocity: design.outlet.triangle.w(),
        }
    }
}

/// Inputs of the geometry steps, one group per design step.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ImpellerGeometryInput {
    pub meridional: MeridionalInput,
    pub properties: BladePropertiesInput,
    pub mean_lines: MeanLineInput,
    pub profiles: ProfileInput,
    pub edges: EdgeInput,
}

/// Complete impeller geometry.
#[derive(Debug, Clone)]
pub struct ImpellerGeometry {
    pub basis: ImpellerBasis,
    pub blade_loading: BladeLoading,
    pub contour: MeridionalContour,
    pub channel: Channel,
    pub blade: Blade,
    pub warnings: Vec<String>,
}

/// Builds the meridional contour, the meridional flow and the blades.
pub fn build(
    basis: &ImpellerBasis,
    input: &ImpellerGeometryInput,
) -> Result<ImpellerGeometry, DesignError> {
    let contour = MeridionalContour::build(basis, &input.meridional)?;
    let spans = input.properties.spans.resolve(|| DEFAULT_SPANS);
    if spans < 2 {
        return Err(DesignError::InvalidInput(
            "at least 2 spans (hub and shroud) are needed".into(),
        ));
    }
    let channel = Channel::build(&contour, basis.flow, QUASI_ORTHOGONALS, spans as usize);
    let blade = Blade::build(
        basis,
        &contour,
        &channel,
        &input.mean_lines,
        &input.profiles,
        &input.edges,
    )?;

    let mut warnings = Vec::new();
    let blade_loading = BladeLoading::evaluate(basis, &blade);
    let ratio = blade_loading.ratio();
    if ratio < 1.0 - BladeLoading::BAND {
        warnings.push(format!(
            "blades are lightly loaded (ξ_eff/ξ_al = {ratio:.2}): fewer or shorter blades \
             reduce friction losses"
        ));
    } else if ratio > 1.0 + BladeLoading::BAND {
        warnings.push(format!(
            "blades are heavily loaded (ξ_eff/ξ_al = {ratio:.2}): more or longer blades \
             reduce the risk of separation"
        ));
    }
    let rise = channel.max_cm_rise();
    if rise > 0.05 {
        warnings.push(format!(
            "the meridional velocity rises by {:.0} % inside the channel; smooth the hub and \
             shroud contours",
            rise * 100.0
        ));
    }
    for s in &blade.sections {
        let wrap = s.wrap_angle().to_degrees();
        if !(45.0..=200.0).contains(&wrap) {
            warnings.push(format!(
                "wrap angle {wrap:.0}° at span {:.2} is outside the usual 45°–200°",
                s.span
            ));
        }
    }
    Ok(ImpellerGeometry {
        basis: *basis,
        blade_loading,
        contour,
        channel,
        blade,
        warnings,
    })
}

/// Hydrodynamic blade loading compared with the allowable value of proven radial pumps
/// (Gülich 2nd ed., Table 7.1, eqs. 7.1.10–7.1.11).
///
/// ξ_eff = 2π·ψ / (η_h·z·L*·(w₁* + w₂*)), ξ_al = (nq,Ref/nq)^0.77 with nq,Ref = 40;
/// starred quantities refer to d₂ (lengths) and u₂ (velocities). Following the book's
/// convention that quantities without subscript belong to the outer streamline, the
/// blade length L and w₁ are taken on the shroud section. Gülich suggests ξ_eff about
/// 10 % below ξ_al, within a ±15 % band.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BladeLoading {
    pub effective: f64,
    pub allowable: f64,
}

impl BladeLoading {
    /// Relative half-width of the acceptable band around ξ_al.
    pub const BAND: f64 = 0.15;

    pub fn evaluate(basis: &ImpellerBasis, blade: &Blade) -> Self {
        let d2 = basis.outlet_diameter;
        let u2 = basis.omega * d2 / 2.0;
        let shroud = blade.sections.last().expect("a blade has sections");
        let le = &shroud.leading_edge;
        let w1 = le.cm.hypot(le.u);
        let length = shroud.mean_line_length() / d2;
        let effective = 2.0 * PI * basis.pressure_coefficient
            / (basis.hydraulic_efficiency
                * f64::from(basis.blade_count)
                * length
                * (w1 + basis.outlet_relative_velocity)
                / u2);
        Self {
            effective,
            allowable: (40.0 / basis.nq).powf(0.77),
        }
    }

    /// ξ_eff / ξ_al.
    pub fn ratio(&self) -> f64 {
        self.effective / self.allowable
    }
}

/// Default values of the geometry steps. None is a published correlation.
pub const GEOMETRY_DEFAULTS: &[CorrelationInfo] = &[
    CorrelationInfo {
        id: "impeller.axial_length",
        description: "axial length L = b₂ + (d₁ − d_n)/2",
        reference: "engineering default",
        verification: Verification::EngineeringDefault,
    },
    CorrelationInfo {
        id: "impeller.contour_tangents",
        description: "Bézier tangent fractions 0.55 for hub and shroud",
        reference: "close to the quarter-ellipse value 0.552",
        verification: Verification::EngineeringDefault,
    },
    CorrelationInfo {
        id: "impeller.leading_edge_position",
        description: "leading edge at 25 % of the hub and 5 % of the shroud length",
        reference: "engineering default",
        verification: Verification::EngineeringDefault,
    },
    CorrelationInfo {
        id: "impeller.edges",
        description: "elliptic 4:1 leading edge (c_p,min 0.41 by Gülich eq. 7.1a), full \
                      thickness to the trailing edge so it can be under-filed",
        reference: "Gülich 2nd ed., pp. 347–349 (profile shapes; the ratio is our choice)",
        verification: Verification::EngineeringDefault,
    },
    CorrelationInfo {
        id: "impeller.blade_loading",
        description: "blade loading ξ_eff = 2π·ψ/(η_h·z·L*·(w₁* + w₂*)) against \
                      ξ_al = (40/nq)^0.77 ± 15 %, L and w₁ on the outer streamline",
        reference: "Gülich 2nd ed., Table 7.1, eqs. 7.1.10 and 7.1.11",
        verification: Verification::Verified,
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pump::{PumpDuty, design};
    use approx::assert_relative_eq;
    use std::f64::consts::PI;

    fn reference_design() -> (CentrifugalPumpDesign, CentrifugalPumpInput) {
        let duty = PumpDuty {
            flow: 100.0 / 3600.0,
            head: 32.0,
            machine_head: 32.0,
            stages: 1,
            speed: 2900.0 * PI / 30.0,
            density: 998.2,
            vapor_pressure: 2339.0,
            inlet_total_pressure: 101_325.0,
        };
        let input = CentrifugalPumpInput::default();
        (design(&duty, &input).unwrap(), input)
    }

    #[test]
    fn basis_from_pump_design() {
        let (d, input) = reference_design();
        let basis = ImpellerBasis::from_pump(&d, &input, 1.0);
        assert_relative_eq!(basis.omega, 2900.0 * PI / 30.0, max_relative = 1e-12);
        assert_eq!(basis.outlet_width, d.outlet_width);
    }

    #[test]
    fn reference_impeller_builds_without_warnings() {
        let (d, input) = reference_design();
        let basis = ImpellerBasis::from_pump(&d, &input, 1.0);
        let g = build(&basis, &ImpellerGeometryInput::default()).unwrap();
        assert!(g.warnings.is_empty(), "{:?}", g.warnings);
        assert_eq!(g.blade.sections.len(), DEFAULT_SPANS as usize);
        assert_eq!(g.blade.count, d.blade_count);
    }

    #[test]
    fn double_entry_models_one_half() {
        let (d, mut input) = reference_design();
        input.suction = crate::pump::Suction::Double;
        let basis = ImpellerBasis::from_pump(&d, &input, 1.0);
        assert_relative_eq!(basis.outlet_width, d.outlet_width / 2.0);
        assert_relative_eq!(basis.flow, d.impeller_flow / 2.0);
    }

    #[test]
    fn blade_loading_of_the_reference_impeller() {
        let (d, input) = reference_design();
        let basis = ImpellerBasis::from_pump(&d, &input, 1.0);
        let g = build(&basis, &ImpellerGeometryInput::default()).unwrap();
        let loading = g.blade_loading;
        // ξ_al = (40/35.92)^0.77
        assert_relative_eq!(
            loading.allowable,
            (40.0 / d.nq).powf(0.77),
            max_relative = 1e-12
        );
        assert!(
            (1.0 - BladeLoading::BAND..1.0 + BladeLoading::BAND).contains(&loading.ratio()),
            "ratio {}",
            loading.ratio()
        );
    }

    #[test]
    fn fewer_blades_carry_more_load() {
        let (_, mut input) = reference_design();
        let duty = PumpDuty {
            flow: 100.0 / 3600.0,
            head: 32.0,
            machine_head: 32.0,
            stages: 1,
            speed: 2900.0 * PI / 30.0,
            density: 998.2,
            vapor_pressure: 2339.0,
            inlet_total_pressure: 101_325.0,
        };
        let ratio = |z: u32, input: &mut CentrifugalPumpInput| {
            input.blade_count = crate::Param::Manual(z);
            let d = design(&duty, input).unwrap();
            let basis = ImpellerBasis::from_pump(&d, input, 1.0);
            build(&basis, &ImpellerGeometryInput::default())
                .unwrap()
                .blade_loading
                .ratio()
        };
        let five = ratio(5, &mut input);
        let seven = ratio(7, &mut input);
        assert!(five > seven, "z=5: {five}, z=7: {seven}");
        // Five blades overload this impeller and are reported.
        let d = {
            input.blade_count = crate::Param::Manual(5);
            design(&duty, &input).unwrap()
        };
        let basis = ImpellerBasis::from_pump(&d, &input, 1.0);
        let g = build(&basis, &ImpellerGeometryInput::default()).unwrap();
        assert!(
            g.warnings.iter().any(|w| w.contains("heavily loaded")),
            "{:?}",
            g.warnings
        );
    }

    #[test]
    fn rejects_single_span() {
        let (d, input) = reference_design();
        let basis = ImpellerBasis::from_pump(&d, &input, 1.0);
        let mut g = ImpellerGeometryInput::default();
        g.properties.spans = crate::Param::Manual(1);
        assert!(build(&basis, &g).is_err());
    }
}
