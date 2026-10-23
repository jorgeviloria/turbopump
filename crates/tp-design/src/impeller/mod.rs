//! Geometry of radial impellers: meridional contour, meridional flow and blades.
//!
//! The geometry steps start from the main dimensions, gathered in [`ImpellerBasis`],
//! so the same code serves any machine type that produces them.

pub mod blade;
pub mod center;
pub mod chambers;
pub mod guide;
pub mod loading;
pub mod material;
pub mod meridian;
pub mod potential;
pub mod wall;

use std::f64::consts::PI;

use serde::{Deserialize, Serialize};
use tp_math::DVec2;

use crate::DesignError;
use crate::pump::correlations::{CorrelationInfo, Verification};
use crate::pump::{CentrifugalPumpDesign, CentrifugalPumpInput};
use blade::{
    Blade, BladePropertiesInput, DEFAULT_SPANS, EdgeInput, MeanLineInput, MeridionalFlow,
    ProfileInput,
};
use meridian::{Channel, MeridionalContour, MeridionalInput};
use potential::PotentialMesh;

/// Quasi-orthogonals used for the meridional flow.
const QUASI_ORTHOGONALS: usize = 81;
/// Nodes across the channel for the potential flow the blades are built on.
const POTENTIAL_ACROSS: usize = 21;

/// Largest difference, in degrees, between the leading-edge angle of a cylindrical blade
/// and the angle its section's inflow asks for before it is reported (our choice, about
/// the width of the recommended incidence range of Gülich Table 7.1).
pub const CYLINDRICAL_INCIDENCE_LIMIT: f64 = 5.0;

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
    /// d₂i at the hub end of the outlet edge, m; d₂ for a radial impeller.
    pub outlet_hub_diameter: f64,
    /// Inclination ε_s of the outer streamline at the outlet to a normal to the axis,
    /// rad; 0 for a radial impeller.
    pub outlet_inclination: f64,
    /// Slip factor γ at the outer streamline.
    pub slip_factor: f64,
    /// c_u2 at d₂ (the outer streamline), m/s.
    pub outlet_swirl: f64,
    /// c_m2 at the outlet, uniform over the outlet edge, m/s.
    pub outlet_meridional_velocity: f64,
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
            outlet_hub_diameter: design.outlet_hub_diameter,
            outlet_inclination: design.outlet_inclination,
            slip_factor: design.outlet.slip_factor,
            outlet_swirl: design.outlet.triangle.cu,
            outlet_meridional_velocity: design.outlet.triangle.cm,
        }
    }

    /// The outlet edge runs from d₂i to d₂a: a semi-axial impeller.
    pub fn is_semi_axial(&self) -> bool {
        self.outlet_hub_diameter < self.outlet_diameter * (1.0 - 1e-9)
    }

    /// Blade outlet angle β₂B at radius `r` of the outlet edge. With the free vortex
    /// c_u·r = const (Gülich Chap. 7.5, Table D7.2 case A) every streamline does the work
    /// of the outer one, and the slip relation c_u/u = γ − c_m·τ/(u·tan β₂B) of the outer
    /// streamline gives tan β₂B = τ·c_m/(γ·u − c_u) at each radius, τ by fixed-point
    /// iteration. A radial impeller keeps β₂B.
    pub fn outlet_blade_angle_at(&self, r: f64) -> Result<f64, DesignError> {
        if !self.is_semi_axial() {
            return Ok(self.outlet_blade_angle);
        }
        let r2 = self.outlet_diameter / 2.0;
        let u = self.omega * r;
        let cu = self.outlet_swirl * r2 / r;
        let cm = self.outlet_meridional_velocity;
        let margin = self.slip_factor * u - cu;
        if margin <= 0.0 {
            return Err(DesignError::Infeasible(format!(
                "with the free vortex the hub streamline at r = {:.1} mm would need an outlet \
                 blade angle above 90°; enlarge the outlet hub diameter",
                r * 1e3
            )));
        }
        let mut beta = self.outlet_blade_angle;
        for _ in 0..50 {
            let tau = crate::triangle::blockage_factor(
                self.blade_count,
                self.blade_thickness,
                2.0 * r,
                beta,
                std::f64::consts::FRAC_PI_2,
            );
            if !tau.is_finite() {
                return Err(DesignError::Infeasible(format!(
                    "the blades fill the outlet circumference at r = {:.1} mm",
                    r * 1e3
                )));
            }
            let next = (tau * cm).atan2(margin);
            let done = (next - beta).abs() < 1e-12;
            beta = next;
            if done {
                break;
            }
        }
        Ok(beta)
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

pub use material::default_wall_thickness;

impl ImpellerGeometry {
    /// The outer faces of the impeller of walls `wall` thick, as the casing beside it sees
    /// them. Toward −z, the front face of the shroud: its contour offset outward by the
    /// wall, from r₂ in to the eye. Toward +z, the rear face of the hub disk, flat at the
    /// outlet plus the wall, from r₂ to the axis; a double-entry impeller, modelled as
    /// one half whose hub is the plane of symmetry, has its other shroud there instead.
    pub fn sides(&self, wall: f64, double_entry: bool) -> crate::outlet::ImpellerSides {
        use crate::outlet::SIDE_POINTS;
        let c = &self.contour;
        // The axial position of the hub at the outlet.
        let length = c.hub.end().x;
        let last = (SIDE_POINTS - 1) as f64;
        // The shroud from the outlet (t = 1) to the eye (t = 0), (z, r), moved outward:
        // to the left of the way from the eye to the outlet.
        let front: [DVec2; SIDE_POINTS] = std::array::from_fn(|i| {
            let t = 1.0 - i as f64 / last;
            let h = 1e-4;
            let a = c.shroud_at((t - h).max(0.0));
            let b = c.shroud_at((t + h).min(1.0));
            let along = (b - a).normalize_or_zero();
            let p = c.shroud_at(t) + DVec2::new(-along.y, along.x) * wall;
            DVec2::new(p.y, p.x)
        });
        // The rear face starts at the hub end of the outlet edge.
        let r2 = self.basis.outlet_hub_diameter / 2.0;
        let plus = if double_entry {
            front.map(|p| DVec2::new(p.x, 2.0 * length - p.y))
        } else {
            std::array::from_fn(|i| DVec2::new(r2 * (1.0 - i as f64 / last), length + wall))
        };
        crate::outlet::ImpellerSides { minus: front, plus }
    }
}

/// Builds the meridional contour, the meridional flow and the blades.
pub fn build(
    basis: &ImpellerBasis,
    input: &ImpellerGeometryInput,
) -> Result<ImpellerGeometry, DesignError> {
    let mut contour = MeridionalContour::build(basis, &input.meridional)?;
    if input.properties.shape.is_cylindrical()
        && input.meridional.leading_edge_hub.is_auto()
        && input.meridional.leading_edge_shroud.is_auto()
    {
        contour.leading_edge = contour.axial_leading_edge();
    }
    let spans = input.properties.spans.resolve(|| DEFAULT_SPANS);
    if spans < 2 {
        return Err(DesignError::InvalidInput(
            "at least 2 spans (hub and shroud) are needed".into(),
        ));
    }
    let mut warnings = contour.material_warnings.clone();
    warnings.extend(contour.chambers.warnings.iter().cloned());
    // The quasi-orthogonals keep the quasi-one-dimensional flow; the blade sections lie
    // on the stream surfaces of the chosen flow, solved on the same stations so that a
    // span point's index stays its channel position.
    let quasi = Channel::build(&contour, basis.flow, QUASI_ORTHOGONALS, spans as usize);
    let channel = match input.properties.meridional_flow {
        MeridionalFlow::QuasiOneDimensional => quasi,
        MeridionalFlow::Potential => {
            let mesh = PotentialMesh {
                stations: QUASI_ORTHOGONALS,
                across: POTENTIAL_ACROSS,
            };
            match potential::solve(&contour, basis.flow, mesh) {
                Ok(flow) => {
                    let spans = quasi
                        .spans
                        .iter()
                        .map(|s| flow.streamline(s.fraction))
                        .collect();
                    quasi.with_spans(spans)
                }
                Err(e) => {
                    warnings.push(format!(
                        "the potential flow failed ({e}); the blades use the \
                         quasi-one-dimensional flow"
                    ));
                    quasi
                }
            }
        }
    };
    let blade = Blade::build(
        basis,
        &contour,
        &channel,
        &input.properties,
        &input.mean_lines,
        &input.profiles,
        &input.edges,
    )?;

    warnings.extend(blade.warnings.iter().cloned());
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
    let shape = input.properties.shape;
    if shape.is_cylindrical() {
        let (off, span) = blade
            .sections
            .iter()
            .map(|s| {
                (
                    (s.beta[0] - s.leading_edge.blade_angle).to_degrees(),
                    s.span,
                )
            })
            .max_by(|a, b| a.0.abs().total_cmp(&b.0.abs()))
            .expect("a blade has sections");
        if off.abs() > CYLINDRICAL_INCIDENCE_LIMIT {
            warnings.push(format!(
                "the cylindrical blade meets the flow {off:+.1}° off its inlet angle at span \
                 {span:.2}; a free-form blade fits every span"
            ));
        }
    }
    if basis.is_semi_axial() {
        let shroud = blade.sections.last().expect("a blade has sections");
        let length = shroud.mean_line_length() / basis.outlet_diameter;
        let minimum = crate::pump::correlations::semi_axial_min_blade_length(
            basis.nq,
            basis.outlet_blade_angle,
        );
        if length < minimum {
            warnings.push(format!(
                "the outer blade is {length:.2}·d2 long, below the {minimum:.2}·d2 a semi-axial \
                 impeller needs for a stable head curve (Gülich eq. 7.17); lengthen the blade \
                 or add wrap"
            ));
        }
    }
    // Straight blades are radial or nearly so and wrap little by design.
    for s in blade
        .sections
        .iter()
        .filter(|_| shape != blade::BladeShape::Straight)
    {
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
pub(crate) mod tests {
    use super::*;
    use crate::pump::{PumpDuty, design};
    use approx::assert_relative_eq;
    use std::f64::consts::{FRAC_PI_2, PI};

    /// Geometry of the reference impeller, every value automatic.
    pub(crate) fn reference_geometry() -> ImpellerGeometry {
        let (d, input) = reference_design();
        let basis = ImpellerBasis::from_pump(&d, &input, 1.0);
        build(&basis, &ImpellerGeometryInput::default()).unwrap()
    }

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
            kinematic_viscosity: 1.0e-6,
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
        for input in [
            ImpellerGeometryInput::default(),
            with_flow(MeridionalFlow::QuasiOneDimensional),
        ] {
            let g = build(&basis, &input).unwrap();
            assert!(g.warnings.is_empty(), "{:?}", g.warnings);
            assert_eq!(g.blade.sections.len(), DEFAULT_SPANS as usize);
            assert_eq!(g.blade.count, d.blade_count);
        }
    }

    fn with_flow(flow: MeridionalFlow) -> ImpellerGeometryInput {
        let mut input = ImpellerGeometryInput::default();
        input.properties.meridional_flow = flow;
        input
    }

    #[test]
    fn potential_flow_sets_the_inlet_angles() {
        let (d, input) = reference_design();
        let basis = ImpellerBasis::from_pump(&d, &input, 1.0);
        let quasi = build(&basis, &with_flow(MeridionalFlow::QuasiOneDimensional)).unwrap();
        let potential = build(&basis, &ImpellerGeometryInput::default()).unwrap();
        let hub = |g: &ImpellerGeometry| g.blade.sections[0].leading_edge;
        let shroud = |g: &ImpellerGeometry| g.blade.sections.last().unwrap().leading_edge;
        // In the bend the shroud, the inner wall, runs faster and the hub slower than
        // the channel average: the shroud blade opens up and the hub blade closes.
        assert!(shroud(&potential).cm > shroud(&quasi).cm * 1.15);
        assert!(hub(&potential).cm < hub(&quasi).cm * 0.85);
        assert!(shroud(&potential).blade_angle > shroud(&quasi).blade_angle + 3f64.to_radians());
        assert!(hub(&potential).blade_angle < hub(&quasi).blade_angle - 3f64.to_radians());
        // The sections lie on the potential stream surfaces and carry their velocity.
        let flow = potential::solve(
            &potential.contour,
            basis.flow,
            PotentialMesh {
                stations: QUASI_ORTHOGONALS,
                across: POTENTIAL_ACROSS,
            },
        )
        .unwrap();
        for (span, section) in potential
            .channel
            .spans
            .iter()
            .zip(&potential.blade.sections)
        {
            assert_eq!(span, &flow.streamline(span.fraction));
            assert_eq!(section.cm.len(), section.m.len());
            assert_relative_eq!(section.cm[0], section.leading_edge.cm, max_relative = 1e-6);
        }
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
        let g = build(&basis, &with_flow(MeridionalFlow::QuasiOneDimensional)).unwrap();
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
        // On the potential flow with a common wrap too.
        let g = build(&basis, &ImpellerGeometryInput::default()).unwrap();
        assert!(
            (1.0 - BladeLoading::BAND..1.0 + BladeLoading::BAND).contains(&g.blade_loading.ratio())
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
            kinematic_viscosity: 1.0e-6,
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

    fn shaped(
        shape: blade::BladeShape,
        basis: &ImpellerBasis,
    ) -> Result<ImpellerGeometry, DesignError> {
        let mut g = ImpellerGeometryInput::default();
        g.properties.shape = shape;
        build(basis, &g)
    }

    /// θ of a section at radius `r`, interpolated between its stations.
    fn theta_at_radius(s: &blade::BladeSection, r: f64) -> f64 {
        let k = s
            .meridional
            .windows(2)
            .position(|w| (w[0].y - r) * (w[1].y - r) <= 0.0)
            .expect("the radius lies on the section");
        let (a, b) = (s.meridional[k].y, s.meridional[k + 1].y);
        let w = if (b - a).abs() < 1e-15 {
            0.0
        } else {
            (r - a) / (b - a)
        };
        s.theta[k] + w * (s.theta[k + 1] - s.theta[k])
    }

    #[test]
    fn cylindrical_blades_share_one_plan_view() {
        use blade::BladeShape::*;
        let (d, input) = reference_design();
        let basis = ImpellerBasis::from_pump(&d, &input, 1.0);
        for shape in [Cylindrical, CircularArc] {
            let g = shaped(shape, &basis).unwrap();
            let r2 = basis.outlet_diameter / 2.0;
            let hub = &g.blade.sections[0];
            let shroud = g.blade.sections.last().unwrap();
            // At the same radius every section sits at the same angle: no lean.
            let r = 0.5
                * (shroud
                    .leading_edge
                    .position
                    .y
                    .max(hub.leading_edge.position.y)
                    + r2);
            assert_relative_eq!(
                theta_at_radius(hub, r),
                theta_at_radius(shroud, r),
                epsilon = 2e-3
            );
            for s in &g.blade.sections {
                assert_relative_eq!(
                    *s.beta.last().unwrap(),
                    basis.outlet_blade_angle,
                    epsilon = 1e-3
                );
            }
            // The middle span meets its inflow exactly.
            let mid = &g.blade.sections[g.blade.sections.len() / 2];
            assert_relative_eq!(mid.beta[0], mid.leading_edge.blade_angle, epsilon = 1e-3);
        }
    }

    #[test]
    fn circular_arc_matches_gulich_eq_7_10() {
        // On a purely radial section the plan view is the section itself: the arc's
        // radius from eq. 7.10, R = (r₂² − r₁²)/(2(r₂ cos β₂ − r₁ cos β₁)), fixes
        // cos β(r) = (r² + R² − a²)/(2rR) with a the distance of its centre from the axis.
        let (r1, b1, r2, b2) = (0.04, 20f64.to_radians(), 0.1, 25f64.to_radians());
        let big_r = (r2 * r2 - r1 * r1) / (2.0 * (r2 * b2.cos() - r1 * b1.cos()));
        let a2 = r2 * r2 + big_r * big_r - 2.0 * r2 * big_r * b2.cos();
        let plan = blade::tests_support::plan(blade::BladeShape::CircularArc, (r1, b1), r2, b2);
        for r in [0.05, 0.07, 0.09] {
            let expected = ((r * r + big_r * big_r - a2) / (2.0 * r * big_r)).acos();
            assert_relative_eq!(plan(r), expected, epsilon = 1e-6);
        }
    }

    #[test]
    fn straight_radial_blades_have_no_wrap() {
        let (d, input) = reference_design();
        let mut basis = ImpellerBasis::from_pump(&d, &input, 1.0);
        // At 22° a straight blade cannot reach the eye.
        assert!(shaped(blade::BladeShape::Straight, &basis).is_err());
        basis.outlet_blade_angle = FRAC_PI_2;
        let g = shaped(blade::BladeShape::Straight, &basis).unwrap();
        for s in &g.blade.sections {
            assert!(s.theta.iter().all(|t| t.abs() < 1e-9));
            assert!(s.beta.iter().all(|b| (b - FRAC_PI_2).abs() < 1e-9));
        }
        // Radial blades meet a swirl-free inflow far off its angle.
        assert!(
            g.warnings.iter().any(|w| w.contains("cylindrical blade")),
            "{:?}",
            g.warnings
        );
    }

    /// A semi-axial impeller: 800 m³/h, 12 m, 1450 rpm (nq ≈ 106).
    pub(crate) fn semi_axial_geometry() -> ImpellerGeometry {
        let duty = PumpDuty {
            flow: 800.0 / 3600.0,
            head: 12.0,
            machine_head: 12.0,
            stages: 1,
            speed: 1450.0 * PI / 30.0,
            density: 998.2,
            vapor_pressure: 2339.0,
            inlet_total_pressure: 101_325.0,
            kinematic_viscosity: 1.0e-6,
        };
        let input = CentrifugalPumpInput::default();
        let d = design(&duty, &input).unwrap();
        assert!(d.semi_axial);
        let basis = ImpellerBasis::from_pump(&d, &input, 1.0);
        build(&basis, &ImpellerGeometryInput::default()).unwrap()
    }

    #[test]
    fn semi_axial_contour_ends_on_an_inclined_outlet_edge() {
        let g = semi_axial_geometry();
        let b = &g.basis;
        let (hub, shroud) = (g.contour.hub.end(), g.contour.shroud.end());
        let expected = DVec2::new(g.contour.axial_length, b.outlet_hub_diameter / 2.0);
        assert!(hub.distance(expected) < 1e-12, "{hub} vs {expected}");
        assert_relative_eq!(shroud.y, b.outlet_diameter / 2.0, max_relative = 1e-12);
        // Both walls leave at ε_s, b₂ apart.
        let eps = b.outlet_inclination;
        let along = DVec2::new(eps.sin(), eps.cos());
        assert!(g.contour.hub.end_direction().distance(along) < 1e-9);
        assert!(g.contour.shroud.end_direction().distance(along) < 1e-9);
        let across = DVec2::new(eps.cos(), -eps.sin());
        assert_relative_eq!(
            (hub - shroud).dot(across),
            b.outlet_width,
            max_relative = 1e-9
        );
    }

    #[test]
    fn semi_axial_blades_follow_the_free_vortex() {
        let g = semi_axial_geometry();
        let b = &g.basis;
        let shroud = g.blade.sections.last().unwrap();
        let hub = &g.blade.sections[0];
        // The outer streamline keeps β₂B; the hub turns more (β₂B,a < β₂B,i, Gülich 7.5).
        assert_relative_eq!(
            *shroud.beta.last().unwrap(),
            b.outlet_blade_angle,
            epsilon = 1e-6
        );
        let hub_beta = *hub.beta.last().unwrap();
        assert!(hub_beta > b.outlet_blade_angle + 2f64.to_radians());
        // c_u·r of the outer streamline at the hub, through the slip relation.
        let r = hub.meridional.last().unwrap().y;
        let u = b.omega * r;
        let cu = b.outlet_swirl * b.outlet_diameter / 2.0 / r;
        let tau = crate::triangle::blockage_factor(
            b.blade_count,
            b.blade_thickness,
            2.0 * r,
            hub_beta,
            FRAC_PI_2,
        );
        assert_relative_eq!(
            hub_beta.tan(),
            tau * b.outlet_meridional_velocity / (b.slip_factor * u - cu),
            max_relative = 1e-9
        );
        // Cylindrical blades need a radial outlet.
        assert!(shaped(blade::BladeShape::CircularArc, b).is_err());
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
