//! Required NPSH of a radial impeller against flow, after Gülich 2nd ed., Chap. 6.3.3.
//!
//! 1. NPSH₃ = λ_c·c₁m²/2g + λ_w·w₁²/2g (eq. 6.10), with c₁m at the eye and w₁ at the
//!    outer streamline. λ_c = 1.1 for axial inflow. At the design point the blade
//!    coefficient comes from the flow angle at the outer streamline,
//!    λ_w,3 = 0.3·(tan β₁a)^0.57 (Fig. 6.20).
//! 2. Below the flow Q_sa the curve follows eq. 6.10 with
//!    λ_w = λ_w,opt·(tan β₁/tan β₁,opt)^0.57 (eq. 6.21).
//! 3. Q_sa marks the steep rise: the smaller of the flow Q_c at which the collector
//!    throat velocity equals c₂u (eq. 6.19) and Q_PS = (Q_SF + Q_W)/2 (eq. 6.20), with
//!    Q_SF the flow of shockless entry (eq. 6.17) and Q_W the flow at which w₁ equals
//!    the velocity w₁q in the inlet throat of the blades (eq. 6.18).
//! 4. Above Q_sa, NPSH₃ = NPSH_sa·(Q/Q_sa)^x with x = 2 to 3 (eq. 6.22).
//!
//! The inflow is taken swirl-free (α₁ = 90°). The inlet throat of the blades is
//! estimated from the main dimensions as the pitch times the sine of the blade angle
//! less the blade thickness at the mean diameter, over the eye width (our estimate).

use std::f64::consts::PI;

use tp_units::STANDARD_GRAVITY as G;

use crate::pump::CentrifugalPumpDesign;

/// λ_c for axial inflow (Gülich Chap. 6.3.2); radial inlet casings take 1.2 to 1.35.
pub const AXIAL_INLET_COEFFICIENT: f64 = 1.1;
/// Exponent x of eq. 6.22, in the middle of Gülich's 2 to 3.
pub const DEFAULT_EXPONENT: f64 = 2.5;
/// Range of flow angles at the outer streamline covered by the data of Fig. 6.20, rad.
pub const BLADE_COEFFICIENT_RANGE: (f64, f64) = (10.0 * PI / 180.0, 25.0 * PI / 180.0);

/// Blade coefficient for a 3 % head drop at q* = 1, from the flow angle β₁a at the
/// outer streamline: λ_w,3 = 0.3·(tan β₁a)^0.57 (Gülich Fig. 6.20, ±25 %).
pub fn blade_coefficient(flow_angle: f64) -> f64 {
    0.3 * flow_angle.tan().max(0.0).powf(0.57)
}

/// NPSH = λ_c·c₁m²/2g + λ_w·w₁²/2g (Gülich eq. 6.10), m.
pub fn npsh(inlet_coefficient: f64, blade_coefficient: f64, cm1: f64, w1: f64) -> f64 {
    (inlet_coefficient * cm1 * cm1 + blade_coefficient * w1 * w1) / (2.0 * G)
}

/// Throat of the collector after the impeller: the volute throat, or the passages
/// between diffuser vanes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CollectorThroat {
    /// z_Le
    pub count: u32,
    /// Area of each throat A₃q, m².
    pub area: f64,
}

/// Flow Q_c at which the mean throat velocity of the collector equals c₂u:
/// Q_c = u₂·A₂·γ / (A₂/(z_Le·A₃q) + τ₂/(η_v·tan β₂B)) (Gülich eq. 6.19), with A₂ the
/// whole outlet area of the impeller, m³/s.
pub fn collector_flow(design: &CentrifugalPumpDesign, throat: CollectorThroat) -> f64 {
    let outlet = &design.outlet;
    let area2 = PI * design.outlet_diameter * design.outlet_width;
    let throats = f64::from(throat.count) * throat.area;
    outlet.triangle.u * area2 * outlet.slip_factor
        / (area2 / throats
            + outlet.blockage / (design.efficiencies.volumetric * outlet.blade_angle.tan()))
}

/// Settings of the NPSH₃ curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NpshSettings {
    /// λ_c
    pub inlet_coefficient: f64,
    /// λ_w at the design point; `None` for Fig. 6.20.
    pub blade_coefficient: Option<f64>,
    /// x of eq. 6.22.
    pub exponent: f64,
}

impl Default for NpshSettings {
    fn default() -> Self {
        Self {
            inlet_coefficient: AXIAL_INLET_COEFFICIENT,
            blade_coefficient: None,
            exponent: DEFAULT_EXPONENT,
        }
    }
}

/// The NPSH₃ curve of one impeller.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct NpshCurve {
    pub inlet_coefficient: f64,
    /// λ_w at the design point.
    pub blade_coefficient: f64,
    /// The flow angle at the outer streamline lies outside the data of Fig. 6.20.
    pub extrapolated: bool,
    pub exponent: f64,
    /// Flow of shockless entry Q_SF, m³/s.
    pub shockless_flow: f64,
    /// Flow Q_W at which w₁ = w₁q; `None` when the blade throat is not narrower than
    /// the eye, m³/s.
    pub throat_flow: Option<f64>,
    /// Flow Q_c at which the collector throat velocity equals c₂u, m³/s.
    pub collector_flow: Option<f64>,
    /// Q_sa, where the steep rise begins, m³/s.
    pub knee_flow: f64,
    /// NPSH₃ at Q_sa, m.
    pub knee_npsh: f64,
    /// NPSH₃ at the design flow, m.
    pub design_npsh: f64,
    design_flow: f64,
    /// Q_La/Q at the design point.
    impeller_ratio: f64,
    /// c₁m at the design flow, m/s.
    design_cm1: f64,
    /// Peripheral speed at the outer streamline of the leading edge, m/s.
    outer_speed: f64,
}

impl NpshCurve {
    /// Builds the curve of an impeller with main dimensions `design` for the design
    /// flow `flow` (m³/s), with the collector `throat` after it if known.
    pub fn new(
        design: &CentrifugalPumpDesign,
        flow: f64,
        throat: Option<CollectorThroat>,
        settings: &NpshSettings,
    ) -> Self {
        let outer = &design.inlet_shroud;
        let mean = &design.inlet_mean;
        let design_cm1 = outer.triangle.cm;
        let outer_speed = outer.triangle.u;
        let impeller_ratio = design.impeller_flow / flow;
        let flow_angle = design_cm1.atan2(outer_speed);
        let blade_coefficient = settings
            .blade_coefficient
            .unwrap_or_else(|| blade_coefficient(flow_angle));
        let (lo, hi) = BLADE_COEFFICIENT_RANGE;
        let extrapolated = settings.blade_coefficient.is_none() && !(lo..=hi).contains(&flow_angle);

        // Pump flow at which the eye carries the meridional velocity `cm1`.
        let flow_at = |cm1: f64| flow * cm1 / design_cm1;
        // Shockless entry at the outer streamline (eq. 6.17 with α₁ = 90°).
        let shockless_flow = flow_at(outer_speed * outer.blade_angle.tan() / outer.blockage);
        // w₁ = w₁q (eq. 6.18 with α₁ = 90°). The inlet throat of the blades is
        // estimated as width × (pitch × sin β₁B − e) at the mean diameter.
        let d1 = design.inlet_diameter;
        let dn = design.hub_diameter;
        let eye_area = PI / 4.0 * (d1 * d1 - dn * dn);
        let throats = 0.5
            * (d1 - dn)
            * (PI * mean.diameter * mean.blade_angle.sin()
                - f64::from(design.blade_count) * design.blade_thickness);
        let throat_flow = (throats > 0.0)
            .then(|| eye_area / throats)
            .filter(|ratio| *ratio > 1.0)
            .map(|ratio| flow_at(mean.triangle.u / (ratio * ratio - 1.0).sqrt()));
        let pressure_side = match throat_flow {
            Some(q) => 0.5 * (shockless_flow + q),
            None => shockless_flow,
        };
        let collector_flow = throat.map(|t| collector_flow(design, t));
        let knee_flow = collector_flow.map_or(pressure_side, |q| q.min(pressure_side));

        let mut curve = Self {
            inlet_coefficient: settings.inlet_coefficient,
            blade_coefficient,
            extrapolated,
            exponent: settings.exponent,
            shockless_flow,
            throat_flow,
            collector_flow,
            knee_flow,
            knee_npsh: 0.0,
            design_npsh: 0.0,
            design_flow: flow,
            impeller_ratio,
            design_cm1,
            outer_speed,
        };
        curve.knee_npsh = curve.below_knee(knee_flow);
        curve.design_npsh = curve.at(flow);
        curve
    }

    /// Eq. 6.10 with the blade coefficient of eq. 6.21.
    fn below_knee(&self, flow: f64) -> f64 {
        let cm1 = self.design_cm1 * flow / self.design_flow;
        let tan_ratio = cm1 / self.design_cm1;
        let lambda_w = self.blade_coefficient * tan_ratio.max(0.0).powf(0.57);
        let w1 = cm1.hypot(self.outer_speed);
        npsh(self.inlet_coefficient, lambda_w, cm1, w1)
    }

    /// NPSH₃ at pump flow `flow` (m³/s), m.
    pub fn at(&self, flow: f64) -> f64 {
        if flow <= self.knee_flow {
            self.below_knee(flow)
        } else {
            self.knee_npsh * (flow / self.knee_flow).powf(self.exponent)
        }
    }

    /// Q_La/Q used to turn the inlet velocities into pump flows.
    pub fn impeller_ratio(&self) -> f64 {
        self.impeller_ratio
    }

    /// The smallest flow up to `max_flow` at which NPSH₃ reaches `available`, m³/s.
    pub fn cavitation_flow(&self, available: f64, max_flow: f64) -> Option<f64> {
        if self.at(max_flow) < available {
            return None;
        }
        let (mut lo, mut hi) = (0.0, max_flow);
        for _ in 0..80 {
            let mid = 0.5 * (lo + hi);
            if self.at(mid) < available {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        Some(hi)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pump::{CentrifugalPumpInput, PumpDuty, design};
    use approx::assert_relative_eq;

    const FT: f64 = 0.3048;

    #[test]
    fn eq_6_10_reproduces_the_npsh_of_the_book_example() {
        // Japikse et al., Fig. 7.73: c₁m = 13.343 ft/s and w₁ = 55.88 ft/s at the inlet
        // tip, a blade cavitation coefficient of 0.200 and NPSH = 12.47 ft.
        let value = npsh(1.0, 0.2, 13.343 * FT, 55.88 * FT);
        assert_relative_eq!(value / FT, 12.47, epsilon = 0.01);
    }

    #[test]
    fn the_blade_coefficient_follows_fig_6_20() {
        // The fitted line of the figure passes about 0.15 at 17° and 0.19 at 24°; at
        // 10° the formula printed with it gives 0.11, a little under the drawn line.
        let at = |deg: f64| blade_coefficient(deg.to_radians());
        assert_relative_eq!(at(10.0), 0.112, epsilon = 0.001);
        assert_relative_eq!(at(17.0), 0.15, epsilon = 0.005);
        assert_relative_eq!(at(24.0), 0.19, epsilon = 0.005);
    }

    fn reference() -> (CentrifugalPumpDesign, f64) {
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
        (
            design(&duty, &CentrifugalPumpInput::default()).unwrap(),
            duty.flow,
        )
    }

    #[test]
    fn the_curve_of_the_reference_pump() {
        let (d, flow) = reference();
        // The reference volute: one throat of 3123 mm².
        let throat = CollectorThroat {
            count: 1,
            area: 3123e-6,
        };
        let curve = NpshCurve::new(&d, flow, Some(throat), &NpshSettings::default());
        // Zero incidence: shockless entry at the design flow.
        assert_relative_eq!(curve.shockless_flow, flow, max_relative = 1e-9);
        // The collector throat reaches c₂u about a quarter above the design flow.
        let qc = curve.collector_flow.unwrap() / flow;
        assert!((1.15..1.35).contains(&qc), "{qc}");
        assert!(curve.knee_flow > flow && curve.knee_flow <= curve.collector_flow.unwrap());
        // At the design point eq. 6.10 with λ_w from Fig. 6.20.
        let outer = &d.inlet_shroud.triangle;
        let lambda = blade_coefficient(outer.cm.atan2(outer.u));
        let expected = npsh(1.1, lambda, outer.cm, outer.cm.hypot(outer.u));
        assert_relative_eq!(curve.design_npsh, expected, max_relative = 1e-9);
        assert!(
            (3.0..6.0).contains(&curve.design_npsh),
            "{}",
            curve.design_npsh
        );
        // The eye of the reference pump is small: its flow angle is above the data.
        assert!(curve.extrapolated);
        // Rising everywhere, continuous at the knee, steeper beyond it.
        let flows: Vec<f64> = (1..=30).map(|i| flow * 0.05 * f64::from(i)).collect();
        assert!(flows.windows(2).all(|w| curve.at(w[1]) > curve.at(w[0])));
        let k = curve.knee_flow;
        assert_relative_eq!(
            curve.at(k * 0.999_999),
            curve.at(k * 1.000_001),
            max_relative = 1e-4
        );
        assert_relative_eq!(
            curve.at(1.2 * k) / curve.knee_npsh,
            1.2f64.powf(DEFAULT_EXPONENT),
            max_relative = 1e-9
        );
        // Low specific speed: NPSH₃ keeps falling at part load (Gülich Fig. 6.23).
        assert!(curve.at(0.6 * flow) < 0.8 * curve.design_npsh);
    }

    #[test]
    fn a_narrow_collector_throat_moves_the_knee_to_lower_flow() {
        let (d, flow) = reference();
        let settings = NpshSettings::default();
        let wide = NpshCurve::new(
            &d,
            flow,
            Some(CollectorThroat {
                count: 1,
                area: 3123e-6,
            }),
            &settings,
        );
        let narrow = NpshCurve::new(
            &d,
            flow,
            Some(CollectorThroat {
                count: 1,
                area: 1500e-6,
            }),
            &settings,
        );
        assert!(narrow.collector_flow.unwrap() < wide.collector_flow.unwrap());
        assert!(narrow.knee_flow < wide.knee_flow);
        assert!(narrow.at(1.3 * flow) > wide.at(1.3 * flow));
    }

    #[test]
    fn cavitation_flow_where_the_curve_meets_the_available_npsh() {
        let (d, flow) = reference();
        let curve = NpshCurve::new(&d, flow, None, &NpshSettings::default());
        let available = curve.at(1.1 * flow);
        let q = curve.cavitation_flow(available, 1.5 * flow).unwrap();
        assert_relative_eq!(q, 1.1 * flow, max_relative = 1e-6);
        assert!(curve.cavitation_flow(100.0, 1.5 * flow).is_none());
    }
}
