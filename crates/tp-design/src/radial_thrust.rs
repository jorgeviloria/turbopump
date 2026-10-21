//! Steady radial force on the impeller against flow, after Gülich 2nd ed., Chap. 9.3
//! and Table 9.4.
//!
//! F_R = k_R·ρ·g·H·d₂·b₂tot (eq. 9.6), with b₂tot the outlet width including both
//! shrouds and q* = Q/Q_opt referred to the design point of the collector.
//! - Single volutes: k_R0 at shut-off from the fit of Fig. 9.27 (curve 1, seal
//!   clearances of eq. 3.12; curve 2 for double clearances), k_R,opt at q* = 1 from
//!   the fit of Fig. 9.28, k_R = (k_R0 − k_R,opt)·(1 − q*²) + k_R,opt below q* = 1 and
//!   k_R,opt·q*² above it (the table's alternative to 0.09·q*²).
//! - Double volutes: k_R = F_Dsp·k_R0 with F_Dsp = 1.75 − 0.0083·ε_sp (ε_sp the wrap
//!   angle of the inner volute in degrees, at most 1; Fig. 9.29), nearly constant up to
//!   q* ≈ 1.1. Below 180° the factor f of the table interpolates toward the single
//!   volute at q* = 0.5 and 1. Above q* = 1.1 the force rises sharply; we continue
//!   with q*² (our choice).
//! - Vaned diffusers: the table gives k_R0 = 0.02 to 0.09 and k_R,opt = 0.01 to 0.06;
//!   we take the middle of each with the law of single volutes (our choice).
//!
//! The unsteady part adds k_R,dyn (Table 9.4, part 2), and for the mechanical design
//! Gülich advises k_R,tot = k_R + k_R,dyn of at least 0.15 (notes 12 and 13).

/// Collectors with radial force data in Table 9.4.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub enum RadialCollector {
    SingleVolute,
    /// Double volute with the inner volute wrapping `wrap` rad.
    DoubleVolute {
        wrap: f64,
    },
    VanedDiffuser,
}

/// k_R0 of single volutes at shut-off against n_q,tot = n_q·√f_q (Fig. 9.27): curve 1
/// for the seal clearances of eq. 3.12, curve 2 for double clearances.
pub fn single_volute_shutoff(nq_tot: f64, wide_clearances: bool) -> f64 {
    let x = nq_tot;
    if wide_clearances {
        -5.0e-5 * x * x + 9.2e-3 * x + 0.191
    } else {
        3.730e-8 * x.powi(4) - 7.274e-6 * x.powi(3) + 3.610e-4 * x * x + 2.041e-3 * x + 3.944e-2
    }
}

/// k_R of single volutes at q* = 1 (Fig. 9.28).
pub fn single_volute_best(nq_tot: f64) -> f64 {
    let x = nq_tot;
    4.562e-5 * x * x - 1.894e-3 * x + 3.179e-2
}

/// k_R of single volutes at q* = 0.5 (Fig. 9.28).
pub fn single_volute_half(nq_tot: f64) -> f64 {
    let x = nq_tot;
    -2.515e-6 * x.powi(3) + 2.633e-4 * x * x - 2.197e-3 * x + 3.429e-2
}

/// F_Dsp = 1.75 − 0.0083·ε_sp, at most 1 (Fig. 9.29); `wrap` in rad.
pub fn double_volute_factor(wrap: f64) -> f64 {
    (1.75 - 0.0083 * wrap.to_degrees()).min(1.0)
}

/// Unsteady k_R,dyn in the middle of the ranges of Table 9.4: below q* = 0.5 and at
/// q* = 1, linear between and constant beyond.
fn dynamic_coefficient(collector: RadialCollector, flow_ratio: f64) -> f64 {
    let (part, best) = match collector {
        RadialCollector::VanedDiffuser => (0.10, 0.05),
        _ => (0.095, 0.03),
    };
    let t = ((flow_ratio - 0.5) / 0.5).clamp(0.0, 1.0);
    part + (best - part) * t
}

/// Least k_R,tot for the mechanical design (Table 9.4, note 13).
pub const MIN_DESIGN_COEFFICIENT: f64 = 0.15;

/// The radial force law of one impeller and collector.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct RadialThrust {
    pub collector: RadialCollector,
    /// k_R at shut-off.
    pub shutoff_coefficient: f64,
    /// k_R at q* = 1.
    pub best_coefficient: f64,
    /// ρ·g·H_opt·d₂·b₂tot, N: the force per unit k_R at the design head.
    pub reference_force: f64,
    /// b₂tot, m.
    pub total_width: f64,
}

impl RadialThrust {
    /// The law for an impeller of specific speed n_q,tot = n_q·√f_q, with
    /// `reference_force` = ρ·g·H_opt·d₂·b₂tot (N) and total outlet width `b2tot` (m).
    pub fn new(
        collector: RadialCollector,
        nq_tot: f64,
        wide_clearances: bool,
        reference_force: f64,
        b2tot: f64,
    ) -> Self {
        let k0_single = single_volute_shutoff(nq_tot, wide_clearances);
        let (shutoff, best) = match collector {
            RadialCollector::SingleVolute => (k0_single, single_volute_best(nq_tot)),
            RadialCollector::DoubleVolute { wrap } => {
                let factor = double_volute_factor(wrap);
                // Interpolation of Table 9.4 toward the single volute below 180°.
                let deg = wrap.to_degrees();
                let f = |single: f64| {
                    if deg >= 180.0 {
                        1.0
                    } else {
                        let ratio = single / k0_single;
                        ratio + (1.0 - ratio) * ((deg - 90.0) / 90.0).max(0.0)
                    }
                };
                (
                    k0_single * factor,
                    k0_single * factor * f(single_volute_best(nq_tot)),
                )
            }
            RadialCollector::VanedDiffuser => (0.055, 0.035),
        };
        Self {
            collector,
            shutoff_coefficient: shutoff,
            best_coefficient: best,
            reference_force,
            total_width: b2tot,
        }
    }

    /// Steady k_R at flow ratio q* = Q/Q_opt.
    pub fn coefficient(&self, flow_ratio: f64) -> f64 {
        let q = flow_ratio.max(0.0);
        let (k0, kopt) = (self.shutoff_coefficient, self.best_coefficient);
        match self.collector {
            // Nearly constant up to q* ≈ 1.1, then rising with q*².
            RadialCollector::DoubleVolute { .. } => {
                let base = if q < 1.0 {
                    (k0 - kopt) * (1.0 - q * q) + kopt
                } else {
                    kopt
                };
                if q > 1.1 {
                    base.max(kopt * (q / 1.1).powi(2))
                } else {
                    base
                }
            }
            _ if q <= 1.0 => (k0 - kopt) * (1.0 - q * q) + kopt,
            _ => kopt * q * q,
        }
    }

    /// Steady radial force at flow ratio `flow_ratio` and head `head` (m), N.
    pub fn force(&self, flow_ratio: f64, head: f64, design_head: f64) -> f64 {
        self.coefficient(flow_ratio) * self.reference_force * head / design_head
    }

    /// k_R,tot = k_R + k_R,dyn for the mechanical design, at least 0.15.
    pub fn design_coefficient(&self, flow_ratio: f64) -> f64 {
        (self.coefficient(flow_ratio) + dynamic_coefficient(self.collector, flow_ratio))
            .max(MIN_DESIGN_COEFFICIENT)
    }
}

/// Default shroud thickness on each side of the outlet, the wall of the CAD model:
/// max(3 mm, 0.02·d₂).
pub fn default_shroud_thickness(d2: f64) -> f64 {
    (0.02 * d2).max(3e-3)
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use std::f64::consts::PI;
    use tp_units::STANDARD_GRAVITY as G;

    #[test]
    fn fits_follow_figs_9_27_and_9_28() {
        // Curve 1 of Fig. 9.27 climbs from about 0.09 at nq = 10 to a peak near 0.37
        // and falls toward 0.28 at nq = 90.
        assert_relative_eq!(single_volute_shutoff(10.0, false), 0.09, epsilon = 0.01);
        let peak = (10..90)
            .map(|n| single_volute_shutoff(f64::from(n), false))
            .fold(0.0, f64::max);
        assert_relative_eq!(peak, 0.37, epsilon = 0.02);
        assert_relative_eq!(single_volute_shutoff(90.0, false), 0.28, epsilon = 0.02);
        // Double clearances lie above.
        assert!(single_volute_shutoff(40.0, true) > single_volute_shutoff(40.0, false));
        // Fig. 9.28: at nq = 40, about 0.20 at q* = 0.5 and 0.025 at q* = 1.
        assert_relative_eq!(single_volute_half(40.0), 0.20, epsilon = 0.01);
        assert_relative_eq!(single_volute_best(40.0), 0.025, epsilon = 0.005);
    }

    #[test]
    fn double_volutes_cut_the_force_and_need_more_than_90_degrees() {
        assert_relative_eq!(double_volute_factor(PI), 1.75 - 0.0083 * 180.0);
        assert_relative_eq!(double_volute_factor(PI / 2.0), 1.0);
        assert!(double_volute_factor(PI) < 0.3);
    }

    fn law(collector: RadialCollector) -> RadialThrust {
        let b2tot = 0.0174 + 2.0 * 3.4e-3;
        RadialThrust::new(
            collector,
            35.9,
            false,
            998.2 * G * 32.0 * 0.1722 * b2tot,
            b2tot,
        )
    }

    #[test]
    fn single_volute_law_of_table_9_4() {
        let t = law(RadialCollector::SingleVolute);
        assert_relative_eq!(t.coefficient(0.0), t.shutoff_coefficient);
        assert_relative_eq!(t.coefficient(1.0), t.best_coefficient);
        assert_relative_eq!(
            t.coefficient(0.5),
            (t.shutoff_coefficient - t.best_coefficient) * 0.75 + t.best_coefficient
        );
        assert_relative_eq!(t.coefficient(1.4), t.best_coefficient * 1.96);
        // Least force near the design point.
        assert!(t.coefficient(1.0) < t.coefficient(0.8) && t.coefficient(1.0) < t.coefficient(1.2));
        assert_relative_eq!(
            t.reference_force,
            998.2 * G * 32.0 * 0.1722 * (0.0174 + 6.8e-3),
            max_relative = 1e-12
        );
        // Shut-off and the head there: F = k_R0·ρ·g·H₀·d₂·b₂tot.
        assert_relative_eq!(
            t.force(0.0, 39.0, 32.0),
            t.shutoff_coefficient * t.reference_force * 39.0 / 32.0
        );
        assert!(t.design_coefficient(1.0) >= MIN_DESIGN_COEFFICIENT);
    }

    #[test]
    fn a_full_double_volute_is_flat_up_to_the_design_point() {
        let single = law(RadialCollector::SingleVolute);
        let double = law(RadialCollector::DoubleVolute { wrap: PI });
        assert_relative_eq!(double.coefficient(0.0), double.coefficient(1.0));
        assert!(double.coefficient(0.0) < 0.3 * single.coefficient(0.0));
        assert!(double.coefficient(1.5) > double.coefficient(1.0));
        // A short rib at 90° is no better than a single volute.
        let short = law(RadialCollector::DoubleVolute { wrap: PI / 2.0 });
        assert_relative_eq!(
            short.coefficient(1.0),
            single.coefficient(1.0),
            max_relative = 1e-9
        );
    }
}
