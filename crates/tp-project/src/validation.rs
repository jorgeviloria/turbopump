//! Comparison of a project's results with reference values.
//!
//! A targets file lists quantities of a reference design (a worked example from a book,
//! or a design computed with another program) and a tolerance for each. [`validate`]
//! computes the same quantities from the project and judges each one.
//!
//! Agreement within a tolerance only means "compared with, within ±x". The tolerances of
//! acceptance-test standards apply to measured pumps; a preliminary-design prediction
//! certifies nothing.
//!
//! Quantities with the same name can be defined differently in different references, so
//! some have two versions. `cm2` uses the impeller flow Q_La = Q/η_v and `cm2_q` the pump
//! flow Q.

use std::cell::OnceCell;

use serde::{Deserialize, Serialize};
use tp_design::impeller::ImpellerGeometry;
use tp_design::impeller::blade::BladeSection;
use tp_design::npsh::{NpshEstimate, NpshMethod};
use tp_design::performance::EfficiencyBreakdown;
use tp_design::performance::PumpPerformance;
use tp_design::pump::CentrifugalPumpDesign;
use tp_design::triangle::VelocityTriangle;
use tp_design::volute::VoluteGeometry;
use tp_units::{QuantityKind, Unit, find_unit};

use crate::{ComponentId, ComponentKind, Project};

/// Value of the `format` field of a targets file.
pub const TARGETS_FORMAT: &str = "turbopump-targets";
/// Version of the targets file format written by this program.
pub const TARGETS_VERSION: u32 = 1;

/// Error reading a targets file.
#[derive(Debug, thiserror::Error)]
pub enum TargetsError {
    #[error("malformed targets file: {0}")]
    Malformed(#[from] serde_json::Error),
    #[error("invalid targets file: {0}")]
    Invalid(String),
}

/// Reference values of one design case.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Targets {
    /// Always [`TARGETS_FORMAT`].
    pub format: String,
    pub version: u32,
    /// Short name of the case.
    #[serde(default)]
    pub case: String,
    #[serde(default)]
    pub description: String,
    /// Free text: provenance, why the tolerances are what they are, known differences.
    #[serde(default)]
    pub notes: Vec<String>,
    pub targets: Vec<Target>,
}

/// One reference value, or a band of values, of one quantity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    /// Quantity id from [`QUANTITIES`]; per-span quantities take `@span`, e.g.
    /// `beta1B@0.25`, `wrap@hub`.
    pub id: String,
    /// Reference value, in `unit`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    /// Range of reference values, in `unit`: the computed value must fall inside.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub band: Option<[f64; 2]>,
    /// Unit of `value`, `band` and an absolute tolerance; empty for a plain number.
    #[serde(default)]
    pub unit: String,
    /// Allowed difference; none makes the target informative.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<Tolerance>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

/// Allowed difference between the computed and the reference value.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tolerance {
    /// In percent of the reference value.
    Relative(f64),
    /// In the unit of the target.
    Absolute(f64),
}

impl Targets {
    /// Reads and checks a targets file.
    pub fn from_json(text: &str) -> Result<Self, TargetsError> {
        let targets: Targets = serde_json::from_str(text)?;
        targets.check()?;
        Ok(targets)
    }

    /// Checks the format, the quantity ids, the units and the tolerances.
    pub fn check(&self) -> Result<(), TargetsError> {
        let invalid = |m: String| Err(TargetsError::Invalid(m));
        if self.format != TARGETS_FORMAT {
            return invalid(format!(
                "format is {:?}, expected {TARGETS_FORMAT:?}",
                self.format
            ));
        }
        if self.version > TARGETS_VERSION {
            return invalid(format!(
                "version {} is newer than this program reads ({TARGETS_VERSION})",
                self.version
            ));
        }
        for t in &self.targets {
            let (info, _) = quantity(&t.id).map_err(TargetsError::Invalid)?;
            target_unit(info.kind, &t.unit)
                .map_err(|e| TargetsError::Invalid(format!("{}: {e}", t.id)))?;
            match (t.value, t.band) {
                (Some(v), None) if v.is_finite() => {}
                (None, Some([lo, hi])) if lo.is_finite() && hi.is_finite() && lo <= hi => {}
                (Some(_), Some(_)) => return invalid(format!("{}: give value or band", t.id)),
                _ => return invalid(format!("{}: needs a finite value or band", t.id)),
            }
            if let Some(Tolerance::Relative(x) | Tolerance::Absolute(x)) = t.tolerance
                && !(x.is_finite() && x >= 0.0)
            {
                return invalid(format!("{}: the tolerance must be positive", t.id));
            }
        }
        Ok(())
    }
}

/// A quantity that a target can refer to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuantityInfo {
    pub id: &'static str,
    pub kind: QuantityKind,
    /// Whether it is given per blade section, with `@span`.
    pub per_span: bool,
    pub description: &'static str,
}

const fn q(id: &'static str, kind: QuantityKind, description: &'static str) -> QuantityInfo {
    QuantityInfo {
        id,
        kind,
        per_span: false,
        description,
    }
}

const fn span(id: &'static str, kind: QuantityKind, description: &'static str) -> QuantityInfo {
    QuantityInfo {
        id,
        kind,
        per_span: true,
        description,
    }
}

use QuantityKind::{
    Angle, Area, Dimensionless as Number, Force, Head, Length, Power, Torque, Velocity, VolumeFlow,
};

/// Every quantity a target can refer to. Main dimensions, geometry and performance are
/// those of the first impeller; volute quantities, of the first volute.
pub const QUANTITIES: &[QuantityInfo] = &[
    // Main dimensions.
    q("nq", Number, "specific speed nq per stage and eye"),
    q("psi", Number, "pressure coefficient ψ = 2gH/u₂²"),
    q("d2", Length, "outlet diameter"),
    q("b2", Length, "outlet width"),
    q("d1", Length, "eye diameter"),
    q("d_hub", Length, "hub diameter at the eye"),
    q("d_shaft", Length, "shaft diameter"),
    q("z", Number, "number of blades"),
    q("e", Length, "blade thickness"),
    q("impeller_flow", VolumeFlow, "impeller flow Q_La = Q/η_v"),
    q("theoretical_head", Head, "theoretical head H_th = H/η_h"),
    q("power", Power, "shaft power of the stage"),
    q("torque", Torque, "shaft torque of the stage"),
    q("eta_h", Number, "hydraulic efficiency"),
    q("eta_v", Number, "volumetric efficiency"),
    q(
        "eta_m",
        Number,
        "mechanical efficiency η/(η_h·η_v), disk friction included",
    ),
    q("eta", Number, "overall efficiency of the stage"),
    q("u2", Velocity, "outlet blade speed"),
    q("cm2", Velocity, "outlet meridional velocity from Q_La"),
    q(
        "cm2_q",
        Velocity,
        "outlet meridional velocity from the pump flow Q",
    ),
    q("cu2", Velocity, "outlet swirl velocity"),
    q("c2", Velocity, "outlet absolute velocity"),
    q("w2", Velocity, "outlet relative velocity"),
    q(
        "alpha2",
        Angle,
        "outlet absolute flow angle from circumferential",
    ),
    q("beta2", Angle, "outlet relative flow angle, from Q_La"),
    q(
        "beta2_q",
        Angle,
        "outlet relative flow angle, from the pump flow Q",
    ),
    q(
        "w2_q",
        Velocity,
        "outlet relative velocity, from the pump flow Q",
    ),
    q("beta2B", Angle, "outlet blade angle"),
    q("slip", Number, "slip factor γ"),
    q("deviation", Angle, "deviation δ₂ = β₂B − β₂"),
    q("cm1", Velocity, "eye meridional velocity from Q_La"),
    q(
        "cm1_q",
        Velocity,
        "eye meridional velocity from the pump flow Q",
    ),
    q(
        "beta1_q",
        Angle,
        "inflow angle at the mean eye diameter, from the pump flow Q",
    ),
    q(
        "w1_shroud",
        Velocity,
        "relative velocity at the eye diameter",
    ),
    q("beta1_shroud", Angle, "inflow angle at the eye diameter"),
    q("beta1B_shroud", Angle, "blade angle at the eye diameter"),
    q("w2_w1", Number, "de Haller ratio w₂/w₁ at the shroud"),
    q("npsh_a", Head, "NPSH available at the inlet"),
    q("npsh3", Head, "NPSH₃ of the main-dimension estimate"),
    q("npsh3.min", Head, "lowest NPSH₃ of the published methods"),
    q("npsh3.max", Head, "highest NPSH₃ of the published methods"),
    q(
        "npsh3.coefficients.low",
        Head,
        "NPSH₃ with λc 1.1 and λw 0.1",
    ),
    q(
        "npsh3.coefficients.high",
        Head,
        "NPSH₃ with λc 1.35 and λw 0.3",
    ),
    q(
        "npsh3.nss.low",
        Head,
        "NPSH₃ at the top nss of the impeller type",
    ),
    q(
        "npsh3.nss.high",
        Head,
        "NPSH₃ at the bottom nss of the impeller type",
    ),
    q("npsh3.stepanoff", Head, "NPSH₃ with Stepanoff's S = 7900"),
    q(
        "npsh3.petermann.low",
        Head,
        "NPSH₃ with Petermann's Sq = 0.45",
    ),
    q(
        "npsh3.petermann.high",
        Head,
        "NPSH₃ with Petermann's Sq = 0.4",
    ),
    q("npsh3.europump.low", Head, "NPSH₃ = 0.3·n·√Q"),
    q("npsh3.europump.high", Head, "NPSH₃ = 0.5·n·√Q"),
    // Impeller geometry.
    q(
        "axial_length",
        Length,
        "axial length of the meridional contour",
    ),
    q("blade_loading", Number, "blade loading ξ_eff/ξ_al"),
    span("le_r", Length, "radius of the leading edge"),
    span("cm_le", Velocity, "meridional velocity at the leading edge"),
    span("beta1", Angle, "inflow angle at the leading edge"),
    span("beta1B", Angle, "blade angle at the leading edge"),
    span("wrap", Angle, "wrap angle of the mean line"),
    span("blade_length", Length, "length of the mean line"),
    // Volute.
    q("volute.cutwater_diameter", Length, "cutwater diameter d_z"),
    q("volute.inlet_width", Length, "volute inlet width b₃"),
    q(
        "volute.throat_area",
        Area,
        "throat area (inner volute of a double one)",
    ),
    q(
        "volute.outer_throat_area",
        Area,
        "throat of the outer channel of a double volute",
    ),
    q("volute.total_throat_area", Area, "sum of the throat areas"),
    q(
        "volute.throat_integral",
        Length,
        "J_sp = Σ∬dA/r over the throats (A/r_c of a single throat, about)",
    ),
    q(
        "volute.throat_velocity",
        Velocity,
        "mean velocity in the throat (of the inner volute in a double one)",
    ),
    q(
        "volute.total_throat_velocity",
        Velocity,
        "the whole volute flow over the sum of the throat areas",
    ),
    q(
        "diffuser.outlet_diameter",
        Length,
        "outlet diameter of the discharge diffuser",
    ),
    q(
        "diffuser.length",
        Length,
        "length of the discharge diffuser",
    ),
    q(
        "diffuser.opening_angle",
        Angle,
        "equivalent opening angle of the discharge diffuser",
    ),
    // Predicted characteristic at the design flow.
    q("perf.head", Head, "head by the loss model"),
    q("perf.power", Power, "shaft power by the loss model"),
    q(
        "perf.efficiency",
        Number,
        "overall efficiency by the loss model",
    ),
    q(
        "perf.hydraulic_efficiency",
        Number,
        "hydraulic efficiency by the loss model",
    ),
    q(
        "perf.volumetric_efficiency",
        Number,
        "volumetric efficiency η_v by the loss model",
    ),
    q(
        "perf.disk_friction_efficiency",
        Number,
        "disk-friction efficiency η_RR = 1 − ΣP_RR/P_i",
    ),
    q(
        "perf.mechanical_efficiency",
        Number,
        "mechanical efficiency η_m = 1 − P_m/P",
    ),
    q(
        "perf.inner_efficiency",
        Number,
        "inner efficiency η_i = η/η_m",
    ),
    q("perf.npsh3", Head, "NPSH₃ of the characteristic"),
    q(
        "perf.throat_best_flow",
        VolumeFlow,
        "best-efficiency flow the volute throat gives (Gülich eq. T4.1.3)",
    ),
    q(
        "perf.throat_best_flow_ratio",
        Number,
        "that flow over the design flow",
    ),
    q("perf.volute_loss", Head, "head lost in the volute"),
    q(
        "perf.volute_swirl",
        Number,
        "swirl parameter λ = c_u/c_m at the volute inlet",
    ),
    q(
        "perf.volute_loss_coefficient",
        Number,
        "spiral loss coefficient of the volute",
    ),
    q("perf.axial_force", Force, "axial thrust on the impeller"),
    q("perf.radial_force", Force, "radial force on the impeller"),
    q("perf.shutoff_head", Head, "head against a closed valve"),
];

/// The quantity named by `id` and its span, for per-span quantities.
fn quantity(id: &str) -> Result<(&'static QuantityInfo, Option<f64>), String> {
    let (name, at) = match id.split_once('@') {
        Some((name, at)) => (name, Some(at)),
        None => (id, None),
    };
    let info = QUANTITIES
        .iter()
        .find(|q| q.id == name)
        .ok_or_else(|| format!("unknown quantity {name:?}"))?;
    let span = match (info.per_span, at) {
        (false, None) => None,
        (false, Some(_)) => return Err(format!("{name} is not given per span")),
        (true, None) => return Err(format!("{name} needs a span, e.g. {name}@0.5")),
        (true, Some(at)) => Some(match at {
            "hub" => 0.0,
            "mid" => 0.5,
            "shroud" => 1.0,
            _ => at
                .parse::<f64>()
                .ok()
                .filter(|s| (0.0..=1.0).contains(s))
                .ok_or_else(|| format!("{id}: the span must be hub, mid, shroud or 0–1"))?,
        }),
    };
    Ok((info, span))
}

/// The unit of a target: `symbol`, or the plain number for an empty dimensionless one.
fn target_unit(kind: QuantityKind, symbol: &str) -> Result<&'static Unit, String> {
    if symbol.is_empty() && kind != QuantityKind::Dimensionless {
        return Err(format!("a {} needs a unit", kind.name()));
    }
    let symbol = if symbol.is_empty() { "-" } else { symbol };
    find_unit(kind, symbol).map_err(|e| e.to_string())
}

/// Outcome of one target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Within the tolerance or the band.
    Pass,
    /// Outside the tolerance or the band.
    Fail,
    /// No tolerance: shown for comparison only.
    Info,
    /// The quantity could not be computed.
    Error,
}

/// One line of a validation report. Values are in the unit of the target.
#[derive(Debug, Clone, PartialEq)]
pub struct ReportRow {
    pub target: Target,
    pub description: &'static str,
    /// Canonical symbol of the target unit.
    pub unit: &'static str,
    pub computed: Option<f64>,
    /// (computed − reference)/|reference| in percent, for a single reference value.
    pub difference_percent: Option<f64>,
    pub verdict: Verdict,
    /// Why the quantity could not be computed.
    pub error: Option<String>,
}

/// Every target of a case with its outcome.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub case: String,
    pub rows: Vec<ReportRow>,
}

impl Report {
    /// Number of rows with verdict `verdict`.
    pub fn count(&self, verdict: Verdict) -> usize {
        self.rows.iter().filter(|r| r.verdict == verdict).count()
    }

    /// Whether every target with a tolerance or a band was met. An informative target
    /// that cannot be computed does not count.
    pub fn passed(&self) -> bool {
        self.rows.iter().all(|r| match r.verdict {
            Verdict::Pass | Verdict::Info => true,
            Verdict::Fail => false,
            Verdict::Error => r.target.tolerance.is_none() && r.target.band.is_none(),
        })
    }
}

/// Computes every target of `targets` from `project` and judges it.
pub fn validate(project: &Project, targets: &Targets) -> Report {
    let results = Results::new(project);
    let rows = targets.targets.iter().map(|t| judge(t, &results)).collect();
    Report {
        case: targets.case.clone(),
        rows,
    }
}

fn judge(target: &Target, results: &Results) -> ReportRow {
    let mut row = ReportRow {
        target: target.clone(),
        description: "",
        unit: "",
        computed: None,
        difference_percent: None,
        verdict: Verdict::Error,
        error: None,
    };
    let computed = quantity(&target.id).and_then(|(info, span)| {
        row.description = info.description;
        let unit = target_unit(info.kind, &target.unit)?;
        row.unit = unit.symbol;
        Ok(unit.from_si(results.value(info.id, span)?))
    });
    let x = match computed {
        Ok(x) => x,
        Err(e) => {
            row.error = Some(e);
            return row;
        }
    };
    row.computed = Some(x);
    row.verdict = match (target.value, target.band) {
        (_, Some([lo, hi])) => {
            if (lo..=hi).contains(&x) {
                Verdict::Pass
            } else {
                Verdict::Fail
            }
        }
        (Some(v), None) => {
            row.difference_percent = (v != 0.0).then(|| 100.0 * (x - v) / v.abs());
            let allowed = match target.tolerance {
                None => {
                    return ReportRow {
                        verdict: Verdict::Info,
                        ..row
                    };
                }
                Some(Tolerance::Relative(p)) => p / 100.0 * v.abs(),
                Some(Tolerance::Absolute(a)) => a,
            };
            // A hair of slack so that a tolerance written as the exact difference passes.
            if (x - v).abs() <= allowed * (1.0 + 1e-9) {
                Verdict::Pass
            } else {
                Verdict::Fail
            }
        }
        (None, None) => Verdict::Error,
    };
    row
}

/// Design results of a project, each computed once when first needed.
struct Results<'a> {
    project: &'a Project,
    impeller: Option<ComponentId>,
    volute: Option<ComponentId>,
    main: OnceCell<Result<CentrifugalPumpDesign, String>>,
    geometry: OnceCell<Result<ImpellerGeometry, String>>,
    volute_geometry: OnceCell<Result<VoluteGeometry, String>>,
    performance: OnceCell<Result<PumpPerformance, String>>,
}

impl<'a> Results<'a> {
    fn new(project: &'a Project) -> Self {
        let first = |kind: ComponentKind| {
            project
                .components()
                .iter()
                .find(|c| c.kind() == kind)
                .map(|c| c.id())
        };
        Self {
            project,
            impeller: first(ComponentKind::Impeller),
            volute: first(ComponentKind::Volute),
            main: OnceCell::new(),
            geometry: OnceCell::new(),
            volute_geometry: OnceCell::new(),
            performance: OnceCell::new(),
        }
    }

    fn impeller(&self) -> Result<ComponentId, String> {
        self.impeller
            .ok_or_else(|| "the project has no impeller".to_string())
    }

    fn main(&self) -> Result<&CentrifugalPumpDesign, String> {
        self.main
            .get_or_init(|| {
                let id = self.impeller()?;
                (self.project.compute_main_dimensions(id)).map_err(|e| e.to_string())
            })
            .as_ref()
            .map_err(Clone::clone)
    }

    fn geometry(&self) -> Result<&ImpellerGeometry, String> {
        self.geometry
            .get_or_init(|| {
                let id = self.impeller()?;
                (self.project.compute_impeller_geometry(id)).map_err(|e| e.to_string())
            })
            .as_ref()
            .map_err(Clone::clone)
    }

    fn volute(&self) -> Result<&VoluteGeometry, String> {
        self.volute_geometry
            .get_or_init(|| {
                let id = self
                    .volute
                    .ok_or_else(|| "the project has no volute".to_string())?;
                self.project.compute_volute(id).map_err(|e| e.to_string())
            })
            .as_ref()
            .map_err(Clone::clone)
    }

    fn performance(&self) -> Result<&PumpPerformance, String> {
        self.performance
            .get_or_init(|| {
                let id = self.impeller()?;
                let input = self
                    .project
                    .performance_input(id)
                    .map_err(|e| e.to_string())?;
                (self.project.compute_performance(id, &input)).map_err(|e| e.to_string())
            })
            .as_ref()
            .map_err(Clone::clone)
    }

    /// The blade section at `span`.
    fn section(&self, span: f64) -> Result<&BladeSection, String> {
        let sections = &self.geometry()?.blade.sections;
        sections
            .iter()
            .find(|s| (s.span - span).abs() < 1e-6)
            .ok_or_else(|| {
                let spans: Vec<String> = sections.iter().map(|s| format!("{}", s.span)).collect();
                format!(
                    "no blade section at span {span}; the spans are {}",
                    spans.join(", ")
                )
            })
    }

    /// Value of quantity `id` in SI.
    fn value(&self, id: &str, span: Option<f64>) -> Result<f64, String> {
        let span = || span.ok_or_else(|| format!("{id} needs a span"));
        // A triangle with the pump flow Q instead of the impeller flow Q_La.
        let with_q = |t: fn(&CentrifugalPumpDesign) -> &VelocityTriangle| {
            self.main().map(|d| d.at_pump_flow(t(d)))
        };
        Ok(match id {
            "nq" => self.main()?.nq,
            "psi" => self.main()?.pressure_coefficient,
            "d2" => self.main()?.outlet_diameter,
            "b2" => self.main()?.outlet_width,
            "d1" => self.main()?.inlet_diameter,
            "d_hub" => self.main()?.hub_diameter,
            "d_shaft" => self.main()?.shaft_diameter,
            "z" => self.main()?.blade_count as f64,
            "e" => self.main()?.blade_thickness,
            "impeller_flow" => self.main()?.impeller_flow,
            "theoretical_head" => self.main()?.theoretical_head,
            "power" => self.main()?.stage_power,
            "torque" => self.main()?.shaft_torque,
            "eta_h" => self.main()?.efficiencies.hydraulic,
            "eta_v" => self.main()?.efficiencies.volumetric,
            "eta_m" => self.main()?.efficiencies.mechanical,
            "eta" => self.main()?.efficiencies.overall,
            "u2" => self.main()?.outlet.triangle.u,
            "cm2" => self.main()?.outlet.triangle.cm,
            "cm2_q" => with_q(|d| &d.outlet.triangle)?.cm,
            "cu2" => self.main()?.outlet.triangle.cu,
            "c2" => self.main()?.outlet.triangle.c(),
            "w2" => self.main()?.outlet.triangle.w(),
            "alpha2" => self.main()?.outlet.triangle.alpha(),
            "beta2" => self.main()?.outlet.triangle.beta(),
            "beta2_q" => with_q(|d| &d.outlet.triangle)?.beta(),
            "w2_q" => with_q(|d| &d.outlet.triangle)?.w(),
            "beta2B" => self.main()?.outlet.blade_angle,
            "slip" => self.main()?.outlet.slip_factor,
            "deviation" => self.main()?.deviation_angle,
            "cm1" => self.main()?.inlet_mean.triangle.cm,
            "cm1_q" => with_q(|d| &d.inlet_mean.triangle)?.cm,
            "beta1_q" => with_q(|d| &d.inlet_mean.triangle)?.beta(),
            "w1_shroud" => self.main()?.inlet_shroud.triangle.w(),
            "beta1_shroud" => self.main()?.inlet_shroud.triangle.beta(),
            "beta1B_shroud" => self.main()?.inlet_shroud.blade_angle,
            "w2_w1" => self.main()?.de_haller,
            "npsh_a" => self.main()?.npsh_available,
            "npsh3" => self.main()?.npsh_required,
            "npsh3.min" => self.main()?.npsh_band.low,
            "npsh3.max" => self.main()?.npsh_band.high,
            "npsh3.coefficients.low" => self.npsh(NpshMethod::InletCoefficients)?.low,
            "npsh3.coefficients.high" => self.npsh(NpshMethod::InletCoefficients)?.high,
            "npsh3.nss.low" => self.npsh(NpshMethod::SuctionSpecificSpeed)?.low,
            "npsh3.nss.high" => self.npsh(NpshMethod::SuctionSpecificSpeed)?.high,
            "npsh3.stepanoff" => self.npsh(NpshMethod::Stepanoff)?.low,
            "npsh3.petermann.low" => self.npsh(NpshMethod::Petermann)?.low,
            "npsh3.petermann.high" => self.npsh(NpshMethod::Petermann)?.high,
            "npsh3.europump.low" => self.npsh(NpshMethod::Europump)?.low,
            "npsh3.europump.high" => self.npsh(NpshMethod::Europump)?.high,
            "axial_length" => self.geometry()?.contour.axial_length,
            "blade_loading" => {
                let l = &self.geometry()?.blade_loading;
                l.effective / l.allowable
            }
            "le_r" => self.section(span()?)?.leading_edge.position.y,
            "cm_le" => self.section(span()?)?.leading_edge.cm,
            "beta1" => self.section(span()?)?.leading_edge.flow_angle,
            "beta1B" => self.section(span()?)?.leading_edge.blade_angle,
            "wrap" => self.section(span()?)?.wrap_angle(),
            "blade_length" => self.section(span()?)?.mean_line_length(),
            "volute.cutwater_diameter" => self.volute()?.design.cutwater_diameter,
            "volute.inlet_width" => self.volute()?.design.inlet_width,
            "volute.throat_area" => self.volute()?.design.throat.area,
            "volute.outer_throat_area" => self.outer_throat()?,
            "volute.total_throat_area" => {
                let inner = self.volute()?.design.throat.area;
                inner + self.outer_throat().unwrap_or(0.0)
            }
            "volute.throat_integral" => self
                .volute()?
                .design
                .throat_integral()
                .map_err(|e| e.to_string())?,
            "volute.total_throat_velocity" => {
                let v = &self.volute()?.design;
                v.flow / (v.throat.area + self.outer_throat().unwrap_or(0.0))
            }
            "volute.throat_velocity" => self.volute()?.design.throat_velocity,
            "diffuser.outlet_diameter" => self.volute()?.design.diffuser.outlet_diameter,
            "diffuser.length" => self.volute()?.design.diffuser.length,
            "diffuser.opening_angle" => self.volute()?.design.diffuser.opening_angle,
            "perf.head" => self.performance()?.design.head,
            "perf.power" => self.performance()?.design.power,
            "perf.efficiency" => self.performance()?.design.efficiency,
            "perf.hydraulic_efficiency" => self.performance()?.design.hydraulic_efficiency,
            "perf.volumetric_efficiency" => self.breakdown()?.volumetric,
            "perf.disk_friction_efficiency" => self.breakdown()?.disk_friction,
            "perf.mechanical_efficiency" => self.breakdown()?.mechanical,
            "perf.inner_efficiency" => self.breakdown()?.inner,
            "perf.throat_best_flow" => self.throat_best_flow()?,
            "perf.throat_best_flow_ratio" => {
                self.throat_best_flow()? / self.performance()?.design.flow
            }
            "perf.npsh3" => self.performance()?.npsh_required,
            "perf.volute_loss" => self.performance()?.design.volute_loss,
            "perf.volute_swirl" => self.volute_summary()?.swirl,
            "perf.volute_loss_coefficient" => self.volute_summary()?.loss_coefficient,
            "perf.axial_force" => self.performance()?.design.axial_force,
            "perf.radial_force" => self.performance()?.design.radial_force,
            "perf.shutoff_head" => self.performance()?.shutoff_head,
            _ => return Err(format!("unknown quantity {id:?}")),
        })
    }

    fn npsh(&self, method: NpshMethod) -> Result<NpshEstimate, String> {
        self.main()?
            .npsh_band
            .estimates
            .iter()
            .find(|e| e.method == method)
            .copied()
            .ok_or_else(|| format!("no NPSH₃ by {}", method.name()))
    }

    fn throat_best_flow(&self) -> Result<f64, String> {
        self.performance()?
            .throat_best_flow
            .ok_or_else(|| "no volute after the impeller".to_string())
    }

    fn breakdown(&self) -> Result<&EfficiencyBreakdown, String> {
        self.performance()?
            .breakdown
            .as_ref()
            .ok_or_else(|| "the prediction uses the statistics, not the loss analysis".to_string())
    }

    fn outer_throat(&self) -> Result<f64, String> {
        let rib = self.volute()?.design.rib.as_ref();
        rib.map(|r| r.outer_throat.area)
            .ok_or_else(|| "the volute is not a double volute".to_string())
    }

    fn volute_summary(&self) -> Result<&tp_design::volute::loss::VoluteLoss, String> {
        self.performance()?
            .volute
            .as_ref()
            .map(|v| &v.design)
            .ok_or_else(|| "the prediction has no volute".to_string())
    }
}

#[cfg(test)]
mod tests {
    use approx::assert_relative_eq;

    use super::*;
    use crate::{DesignPoint, EnergyTransfer, FlowRate, GlobalSetup, MachineType};

    /// 100 m³/h, 32 m, 2900 rpm with an impeller and a volute, all automatic.
    fn pump() -> Project {
        let setup = GlobalSetup::new(
            MachineType::Pump,
            DesignPoint {
                flow: FlowRate::Volume(100.0 / 3600.0),
                energy: EnergyTransfer::Head(32.0),
                speed: 2900.0 * std::f64::consts::PI / 30.0,
            },
        );
        let mut p = Project::new("validation", MachineType::Pump, setup).unwrap();
        p.add_component(ComponentKind::Impeller, None).unwrap();
        p.add_component(ComponentKind::Volute, None).unwrap();
        p
    }

    fn targets(list: Vec<Target>) -> Targets {
        Targets {
            format: TARGETS_FORMAT.into(),
            version: TARGETS_VERSION,
            case: "test".into(),
            description: String::new(),
            notes: Vec::new(),
            targets: list,
        }
    }

    fn target(id: &str, value: f64, unit: &str, tolerance: Option<Tolerance>) -> Target {
        Target {
            id: id.into(),
            value: Some(value),
            band: None,
            unit: unit.into(),
            tolerance,
            note: String::new(),
        }
    }

    #[test]
    fn every_listed_quantity_is_computed() {
        let p = pump();
        let results = Results::new(&p);
        for info in QUANTITIES {
            let span = info.per_span.then_some(0.5);
            let value = results.value(info.id, span);
            // A single volute has no outer channel; everything else must compute.
            if info.id == "volute.outer_throat_area" {
                assert!(value.is_err());
            } else {
                let v = value.unwrap_or_else(|e| panic!("{}: {e}", info.id));
                assert!(v.is_finite(), "{}: {v}", info.id);
            }
        }
    }

    #[test]
    fn relative_absolute_band_and_informative_targets() {
        let p = pump();
        let d2 = p
            .compute_main_dimensions(p.components()[0].id())
            .unwrap()
            .outlet_diameter;
        let mm = 1000.0 * d2;
        let mut band = target("npsh3", 0.0, "m", None);
        band.value = None;
        band.band = Some([0.1, 50.0]);
        let report = validate(
            &p,
            &targets(vec![
                target("d2", mm * 1.02, "mm", Some(Tolerance::Relative(3.0))),
                target("d2", mm * 1.02, "mm", Some(Tolerance::Relative(1.0))),
                target("d2", mm + 0.5, "mm", Some(Tolerance::Absolute(0.5))),
                target("d2", mm, "mm", None),
                band,
            ]),
        );
        let verdicts: Vec<Verdict> = report.rows.iter().map(|r| r.verdict).collect();
        use Verdict::*;
        assert_eq!(verdicts, [Pass, Fail, Pass, Info, Pass]);
        let first = &report.rows[0];
        assert_eq!(first.unit, "mm");
        assert_relative_eq!(first.computed.unwrap(), mm, max_relative = 1e-12);
        assert_relative_eq!(
            first.difference_percent.unwrap(),
            100.0 * (1.0 / 1.02 - 1.0),
            max_relative = 1e-9
        );
        assert!(!report.passed());
        assert_eq!(report.count(Fail), 1);
    }

    #[test]
    fn the_pump_flow_versions_scale_with_the_volumetric_efficiency() {
        let p = pump();
        let results = Results::new(&p);
        let eta_v = results.value("eta_v", None).unwrap();
        let cm2 = results.value("cm2", None).unwrap();
        assert_relative_eq!(
            results.value("cm2_q", None).unwrap(),
            cm2 * eta_v,
            max_relative = 1e-12
        );
        let cm1 = results.value("cm1", None).unwrap();
        assert_relative_eq!(
            results.value("cm1_q", None).unwrap(),
            cm1 * eta_v,
            max_relative = 1e-12
        );
        // Less meridional velocity at the same swirl: a flatter flow angle.
        assert!(results.value("beta2_q", None).unwrap() < results.value("beta2", None).unwrap());
    }

    #[test]
    fn an_uncomputable_target_fails_only_with_a_tolerance() {
        let p = pump();
        let report = validate(
            &p,
            &targets(vec![target(
                "volute.outer_throat_area",
                1000.0,
                "mm²",
                None,
            )]),
        );
        assert_eq!(report.rows[0].verdict, Verdict::Error);
        assert!(report.passed());
        let report = validate(
            &p,
            &targets(vec![target(
                "volute.outer_throat_area",
                1000.0,
                "mm²",
                Some(Tolerance::Relative(5.0)),
            )]),
        );
        assert!(!report.passed());
    }

    #[test]
    fn spans_by_name_or_fraction() {
        assert_eq!(quantity("wrap@hub").unwrap().1, Some(0.0));
        assert_eq!(quantity("wrap@0.25").unwrap().1, Some(0.25));
        assert_eq!(quantity("wrap@shroud").unwrap().1, Some(1.0));
        assert!(quantity("wrap").is_err());
        assert!(quantity("wrap@1.5").is_err());
        assert!(quantity("d2@hub").is_err());
        let p = pump();
        let results = Results::new(&p);
        let err = results.value("wrap", Some(0.3)).unwrap_err();
        assert!(err.contains("no blade section at span 0.3"), "{err}");
    }

    #[test]
    fn the_file_is_checked_when_read() {
        let ok = r#"{"format": "turbopump-targets", "version": 1, "targets": [
            {"id": "b2", "value": 17.0, "unit": "mm", "tolerance": {"relative": 3}},
            {"id": "eta", "value": 75.9, "unit": "%", "tolerance": {"absolute": 2}},
            {"id": "npsh3", "band": [1.9, 5.12], "unit": "m"},
            {"id": "psi", "value": 1.07}
        ]}"#;
        let t = Targets::from_json(ok).unwrap();
        assert_eq!(t.targets[0].tolerance, Some(Tolerance::Relative(3.0)));
        let bad = [
            r#"{"format": "other", "version": 1, "targets": []}"#,
            r#"{"format": "turbopump-targets", "version": 9, "targets": []}"#,
            r#"{"format": "turbopump-targets", "version": 1, "targets": [{"id": "x", "value": 1}]}"#,
            r#"{"format": "turbopump-targets", "version": 1, "targets": [{"id": "b2", "value": 1}]}"#,
            r#"{"format": "turbopump-targets", "version": 1, "targets": [{"id": "b2", "value": 1, "unit": "kg"}]}"#,
            r#"{"format": "turbopump-targets", "version": 1, "targets": [{"id": "b2", "unit": "mm"}]}"#,
            r#"{"format": "turbopump-targets", "version": 1, "targets": [{"id": "b2", "value": 1, "unit": "mm", "tolerance": {"relative": -1}}]}"#,
            r#"{"format": "turbopump-targets", "version": 1, "targets": [{"id": "b2", "band": [2, 1], "unit": "mm"}]}"#,
        ];
        for text in bad {
            assert!(Targets::from_json(text).is_err(), "{text}");
        }
    }
}
