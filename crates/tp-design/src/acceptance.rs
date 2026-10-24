//! Acceptance classes of a pump's hydraulic performance.
//!
//! ISO 9906 sets how far an acceptance test may find the flow, head, power and
//! efficiency from the guaranteed ones. Gülich (*Centrifugal Pumps*, 2nd ed., Table
//! 15.2) quotes those tolerances as three quality classes, G1 to G3, and places a pump
//! in one by its outlet and inlet tip speeds, its head per stage and its power: the
//! most stringent criterion decides.
//!
//! The editor borrows the head tolerance to judge the predicted head against the design
//! point. That is an analogy: the standard compares a measured pump with its guarantee.

use serde::{Deserialize, Serialize};

use crate::Param;
use crate::performance::PumpPerformance;
use crate::pump::CentrifugalPumpDesign;

/// Quality class of a pump, from the most demanding (G1) to the least (G3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityClass {
    G1,
    G2,
    G3,
}

/// Acceptance tolerances of a class, as fractions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tolerances {
    /// ± on the flow rate.
    pub flow: f64,
    /// ± on the head.
    pub head: f64,
    /// Allowed excess of the power consumption.
    pub power: f64,
    /// Allowed shortfall of the efficiency. Table 15.2 prints "0 (−3 %)" for G1; the
    /// bracketed value is kept here.
    pub efficiency: f64,
}

impl QualityClass {
    pub const ALL: [QualityClass; 3] = [QualityClass::G1, QualityClass::G2, QualityClass::G3];

    /// Tolerances on flow, head, power and efficiency (Gülich, Table 15.2, after ISO 9906).
    pub fn tolerances(self) -> Tolerances {
        match self {
            QualityClass::G1 => Tolerances {
                flow: 0.05,
                head: 0.03,
                power: 0.04,
                efficiency: 0.03,
            },
            QualityClass::G2 => Tolerances {
                flow: 0.08,
                head: 0.05,
                power: 0.08,
                efficiency: 0.05,
            },
            QualityClass::G3 => Tolerances {
                flow: 0.09,
                head: 0.07,
                power: 0.09,
                efficiency: 0.07,
            },
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            QualityClass::G1 => "G1",
            QualityClass::G2 => "G2",
            QualityClass::G3 => "G3",
        }
    }
}

/// What places a pump in a quality class.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClassCriteria {
    /// Circumferential velocity at the impeller outlet u₂, m/s.
    pub outlet_tip_speed: f64,
    /// Circumferential velocity at the impeller eye u₁ (outer streamline), m/s.
    pub inlet_tip_speed: f64,
    /// Head per stage at the best efficiency point, m.
    pub head_per_stage: f64,
    /// Power consumption, W.
    pub power: f64,
}

impl ClassCriteria {
    /// The criteria of a designed impeller. The head per stage is the predicted one at
    /// the best efficiency point when there is a prediction, else the design head, and
    /// the power is the predicted one at the design point, else the designed one.
    pub fn of(design: &CentrifugalPumpDesign, performance: Option<&PumpPerformance>) -> Self {
        Self {
            outlet_tip_speed: design.outlet.triangle.u,
            inlet_tip_speed: design.inlet_shroud.triangle.u,
            head_per_stage: performance.map_or(
                design.theoretical_head * design.efficiencies.hydraulic,
                |p| p.best.head,
            ),
            power: performance.map_or(design.stage_power, |p| p.design.power),
        }
    }

    /// The class of each criterion, in the order of the fields (Gülich, Table 15.2).
    pub fn classes(&self) -> [QualityClass; 4] {
        [
            band(self.outlet_tip_speed, 90.0, 40.0),
            band(self.inlet_tip_speed, 50.0, 15.0),
            band(self.head_per_stage, 400.0, 80.0),
            band(self.power, 3.0e6, 3.0e5),
        ]
    }

    /// The class of the pump: the most stringent of its criteria.
    pub fn class(&self) -> QualityClass {
        self.classes().into_iter().min().unwrap_or(QualityClass::G3)
    }
}

/// G1 above `g1`, G2 from `g2` up to `g1`, G3 below `g2`.
fn band(value: f64, g1: f64, g2: f64) -> QualityClass {
    if value > g1 {
        QualityClass::G1
    } else if value >= g2 {
        QualityClass::G2
    } else {
        QualityClass::G3
    }
}

/// How the project judges the predicted performance against the design point.
///
/// It is not a design input: changing it leaves the design as it is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct AcceptanceInput {
    /// Automatic: from [`ClassCriteria::class`].
    #[serde(default)]
    pub class: Param<QualityClass>,
    /// ± on the head, as a fraction. Automatic: the one of the class.
    #[serde(default)]
    pub head_tolerance: Param<f64>,
}

/// The resolved acceptance settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Acceptance {
    pub class: QualityClass,
    pub head_tolerance: f64,
}

impl AcceptanceInput {
    /// The class and head tolerance in use. Without `criteria` an automatic class
    /// cannot be found, and there is none.
    pub fn resolve(&self, criteria: Option<&ClassCriteria>) -> Option<Acceptance> {
        let class = match self.class {
            Param::Manual(class) => class,
            Param::Auto => criteria?.class(),
        };
        Some(Acceptance {
            class,
            head_tolerance: self.head_tolerance.resolve(|| class.tolerances().head),
        })
    }
}

impl Acceptance {
    /// Whether `predicted` is within the head tolerance of `target`.
    pub fn head_within(&self, predicted: f64, target: f64) -> bool {
        (predicted - target).abs() <= self.head_tolerance * target.abs()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn criteria(u2: f64, u1: f64, h: f64, p: f64) -> ClassCriteria {
        ClassCriteria {
            outlet_tip_speed: u2,
            inlet_tip_speed: u1,
            head_per_stage: h,
            power: p,
        }
    }

    #[test]
    fn head_tolerances_of_table_15_2() {
        let heads: Vec<f64> = QualityClass::ALL
            .iter()
            .map(|c| c.tolerances().head)
            .collect();
        assert_eq!(heads, [0.03, 0.05, 0.07]);
        assert_eq!(QualityClass::G2.tolerances().flow, 0.08);
        assert_eq!(QualityClass::G3.tolerances().power, 0.09);
    }

    #[test]
    fn the_bands_include_their_lower_limit_in_g2() {
        assert_eq!(band(90.0, 90.0, 40.0), QualityClass::G2);
        assert_eq!(band(90.1, 90.0, 40.0), QualityClass::G1);
        assert_eq!(band(40.0, 90.0, 40.0), QualityClass::G2);
        assert_eq!(band(39.9, 90.0, 40.0), QualityClass::G3);
    }

    #[test]
    fn the_most_stringent_criterion_decides() {
        // A small pump: every criterion in G3.
        assert_eq!(criteria(26.2, 11.7, 33.7, 10.3e3).class(), QualityClass::G3);
        // The same pump at 400 kW: power alone moves it to G2.
        assert_eq!(criteria(26.2, 11.7, 33.7, 4.0e5).class(), QualityClass::G2);
        // A fast eye alone makes it G1.
        assert_eq!(criteria(26.2, 55.0, 33.7, 10.3e3).class(), QualityClass::G1);
        assert_eq!(
            criteria(95.0, 20.0, 500.0, 2.0e5).classes(),
            [
                QualityClass::G1,
                QualityClass::G2,
                QualityClass::G1,
                QualityClass::G3
            ]
        );
    }

    #[test]
    fn automatic_and_manual_settings() {
        let small = criteria(26.2, 11.7, 33.7, 10.3e3);
        let auto = AcceptanceInput::default();
        assert_eq!(
            auto.resolve(Some(&small)),
            Some(Acceptance {
                class: QualityClass::G3,
                head_tolerance: 0.07
            })
        );
        assert_eq!(auto.resolve(None), None);
        let class = AcceptanceInput {
            class: Param::Manual(QualityClass::G1),
            ..AcceptanceInput::default()
        };
        assert_eq!(class.resolve(None).unwrap().head_tolerance, 0.03);
        let tolerance = AcceptanceInput {
            head_tolerance: Param::Manual(0.02),
            ..AcceptanceInput::default()
        };
        let resolved = tolerance.resolve(Some(&small)).unwrap();
        assert_eq!(resolved.class, QualityClass::G3);
        assert_eq!(resolved.head_tolerance, 0.02);
    }

    #[test]
    fn head_within_the_tolerance() {
        let g3 = Acceptance {
            class: QualityClass::G3,
            head_tolerance: 0.07,
        };
        assert!(g3.head_within(32.25, 32.0));
        assert!(g3.head_within(34.2, 32.0));
        assert!(!g3.head_within(34.3, 32.0));
        assert!(!g3.head_within(29.7, 32.0));
    }

    #[test]
    fn serialized_form() {
        let input = AcceptanceInput {
            class: Param::Manual(QualityClass::G2),
            head_tolerance: Param::Auto,
        };
        let json = serde_json::to_string(&input).unwrap();
        assert_eq!(
            json,
            r#"{"class":{"mode":"manual","value":"g2"},"head_tolerance":{"mode":"auto"}}"#
        );
        assert_eq!(
            serde_json::from_str::<AcceptanceInput>("{}").unwrap(),
            AcceptanceInput::default()
        );
    }
}
