//! Shapes of the blade edges: how the blade thins to its leading and trailing edges,
//! face by face, so that a nose can also lean toward one face.
//!
//! Along the mean line from the edge (s = 0) inward, a shape gives the offset of each
//! face from the mean line as a fraction of the local half thickness: +1 on the
//! pressure face and −1 on the suction face once the shape has ended. Its lengths are
//! in blade thicknesses e of that edge, so that one shape suits every span.

use serde::{Deserialize, Serialize};
use tp_math::{Bezier, DVec2};

/// Samples of a nose curve.
const SAMPLES: usize = 1000;

/// Samples of a Bézier nose while point 2 is placed.
const SEARCH: usize = 300;

/// Shape of a blade edge.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EdgeShape {
    /// Blunt: full thickness up to the edge.
    Cut,
    /// Elliptic nose whose length along the mean line is `ratio` times the half
    /// thickness (1 = round).
    Ellipse { ratio: f64 },
    /// The thickness falls linearly over `length` to a rounded nose `nose` long, as wide
    /// there as `nose / ratio` to each side. The rounding is a conic tangent to the
    /// taper: a quarter ellipse of axis ratio `ratio` when the taper is parallel.
    /// `asymmetry` moves the tip toward the pressure face (+) or the suction face (−),
    /// as a fraction of the half width of the nose, fading out over `length`. Lengths
    /// in blade thicknesses.
    Taper {
        length: f64,
        nose: f64,
        ratio: f64,
        asymmetry: f64,
    },
    /// A degree-4 Bézier curve from the pressure face round the edge to the suction
    /// face. Points 0 and 4 lie on the faces `start` from the edge, points 1 and 3 on
    /// their tangents `pull` of the way toward the edge, and point 2 `tip` half
    /// thicknesses off the mean line, as far from the edge as makes the nose reach it.
    /// `symmetric` takes the pressure-face values for both faces and keeps point 2 on
    /// the mean line. Lengths in blade thicknesses; [pressure, suction].
    Bezier {
        start: [f64; 2],
        pull: [f64; 2],
        tip: f64,
        #[serde(default)]
        symmetric: bool,
    },
}

impl EdgeShape {
    /// A taper over three thicknesses to a nose of 0.6 e and 2 : 1.
    pub const TAPER: EdgeShape = EdgeShape::Taper {
        length: 3.0,
        nose: 0.6,
        ratio: 2.0,
        asymmetry: 0.0,
    };

    /// A symmetric Bézier nose two thicknesses long, close to a 4 : 1 ellipse.
    pub const BEZIER: EdgeShape = EdgeShape::Bezier {
        start: [2.0, 2.0],
        pull: [0.6, 0.6],
        tip: 0.0,
        symmetric: true,
    };

    /// Whether `other` is the same kind of shape (and so can be blended with it).
    pub fn same_kind(&self, other: &EdgeShape) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }

    /// A symmetric Bézier written out for both faces; other shapes as they are.
    pub fn resolved(self) -> EdgeShape {
        match self {
            EdgeShape::Bezier {
                start,
                pull,
                symmetric: true,
                ..
            } => EdgeShape::Bezier {
                start: [start[0]; 2],
                pull: [pull[0]; 2],
                tip: 0.0,
                symmetric: false,
            },
            other => other,
        }
    }

    /// `self` at f = 0 and `other` at f = 1, parameter by parameter; `None` for shapes
    /// of different kinds.
    pub fn lerp(self, other: EdgeShape, f: f64) -> Option<EdgeShape> {
        let mix = |a: f64, b: f64| a + (b - a) * f;
        let pair = |a: [f64; 2], b: [f64; 2]| [mix(a[0], b[0]), mix(a[1], b[1])];
        Some(match (self.resolved(), other.resolved()) {
            (EdgeShape::Cut, EdgeShape::Cut) => EdgeShape::Cut,
            (EdgeShape::Ellipse { ratio: a }, EdgeShape::Ellipse { ratio: b }) => {
                EdgeShape::Ellipse { ratio: mix(a, b) }
            }
            (
                EdgeShape::Taper {
                    length: l0,
                    nose: n0,
                    ratio: r0,
                    asymmetry: a0,
                },
                EdgeShape::Taper {
                    length: l1,
                    nose: n1,
                    ratio: r1,
                    asymmetry: a1,
                },
            ) => EdgeShape::Taper {
                length: mix(l0, l1),
                nose: mix(n0, n1),
                ratio: mix(r0, r1),
                asymmetry: mix(a0, a1),
            },
            (
                EdgeShape::Bezier {
                    start: s0,
                    pull: p0,
                    tip: t0,
                    ..
                },
                EdgeShape::Bezier {
                    start: s1,
                    pull: p1,
                    tip: t1,
                    ..
                },
            ) => {
                let symmetric = matches!(
                    (self, other),
                    (
                        EdgeShape::Bezier {
                            symmetric: true,
                            ..
                        },
                        EdgeShape::Bezier {
                            symmetric: true,
                            ..
                        }
                    )
                );
                EdgeShape::Bezier {
                    start: pair(s0, s1),
                    pull: pair(p0, p1),
                    tip: mix(t0, t1),
                    symmetric,
                }
            }
            _ => return None,
        })
    }

    /// Length of the shaped part along the mean line, in blade thicknesses; `None` for
    /// a blunt edge.
    pub fn nose_length(&self) -> Option<f64> {
        match self.resolved() {
            EdgeShape::Cut => None,
            EdgeShape::Ellipse { ratio } => Some(ratio / 2.0),
            EdgeShape::Taper { length, .. } => Some(length),
            EdgeShape::Bezier { start, .. } => Some(start[0].max(start[1])),
        }
    }

    /// Minimum pressure coefficient c_p,min = Δp_min/(½ρw₁²) of the leading-edge
    /// profile at shockless entry: c_p = 2x + x² with x = 0.373·e/L_p, L_p being the
    /// length of the nose (Gülich 2nd ed., eq. 7.1a, for elliptic noses; for the other
    /// shapes an estimate with the length they take). `None` for a blunt edge.
    pub fn min_pressure_coefficient(&self) -> Option<f64> {
        self.nose_length().map(|length| {
            let x = 0.373 / length;
            2.0 * x + x * x
        })
    }

    /// Checks the parameters; the message names what is wrong.
    pub fn validate(&self) -> Result<(), String> {
        let positive = |name: &str, v: f64| {
            if v.is_finite() && v > 0.0 {
                Ok(())
            } else {
                Err(format!("edge {name} must be positive, got {v}"))
            }
        };
        match self.resolved() {
            EdgeShape::Cut => Ok(()),
            EdgeShape::Ellipse { ratio } => positive("ellipse ratio", ratio),
            EdgeShape::Taper {
                length,
                nose,
                ratio,
                asymmetry,
            } => {
                positive("taper length", length)?;
                positive("nose length", nose)?;
                positive("nose ratio", ratio)?;
                if nose >= length {
                    return Err(format!(
                        "the edge nose ({nose} e) must be shorter than its taper ({length} e)"
                    ));
                }
                if 2.0 * nose / ratio >= 1.0 {
                    return Err(format!(
                        "the edge nose is as wide as the blade: with {nose} e it needs a \
                         ratio above {}",
                        2.0 * nose
                    ));
                }
                if !(asymmetry.is_finite() && asymmetry.abs() < 1.0) {
                    return Err(format!(
                        "the edge asymmetry must lie between −1 and 1, got {asymmetry}"
                    ));
                }
                Ok(())
            }
            EdgeShape::Bezier {
                start, pull, tip, ..
            } => {
                for s in start {
                    positive("Bézier length", s)?;
                }
                for p in pull {
                    if !(0.0..=1.0).contains(&p) {
                        return Err(format!(
                            "the pull of a Bézier edge lies between 0 and 1, got {p}"
                        ));
                    }
                }
                if !(tip.is_finite() && tip.abs() < 1.0) {
                    return Err(format!(
                        "the tip of a Bézier edge lies within the blade (−1 to 1), got {tip}"
                    ));
                }
                Ok(())
            }
        }
    }
}

/// The faces of an edge shape for one thickness: (s, n) along and across the mean
/// line, s in m from the edge and n in half thicknesses.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeProfile {
    length: f64,
    kind: Kind,
}

#[derive(Debug, Clone, PartialEq)]
enum Kind {
    Blunt,
    Ellipse,
    /// Both faces sampled from the tip, s increasing.
    Faces {
        pressure: Vec<DVec2>,
        suction: Vec<DVec2>,
    },
}

/// n at `s` on a face sampled with s increasing; its last value beyond it.
fn face_at(face: &[DVec2], s: f64) -> f64 {
    let i = face.partition_point(|p| p.x < s);
    if i == 0 {
        return face[0].y;
    }
    if i == face.len() {
        return face[face.len() - 1].y;
    }
    let (a, b) = (face[i - 1], face[i]);
    if b.x - a.x <= f64::EPSILON {
        return b.y;
    }
    a.y + (b.y - a.y) * (s - a.x) / (b.x - a.x)
}

/// Splits a nose curve, sampled from the pressure face round to the suction face, at
/// its point nearest the edge into the two faces, each from the tip with s increasing.
fn split_faces(points: &[DVec2]) -> Result<(Vec<DVec2>, Vec<DVec2>), String> {
    let tip = points
        .iter()
        .enumerate()
        .min_by(|a, b| a.1.x.total_cmp(&b.1.x))
        .map_or(0, |(i, _)| i);
    let pressure: Vec<DVec2> = points[..=tip].iter().rev().copied().collect();
    let suction: Vec<DVec2> = points[tip..].to_vec();
    let reach = points.iter().fold(0.0f64, |m, p| m.max(p.x));
    for face in [&pressure, &suction] {
        if face.windows(2).any(|w| w[1].x < w[0].x - 1e-9 * reach) {
            return Err("the edge nose folds back: move its points".into());
        }
    }
    if pressure
        .iter()
        .skip(1)
        .any(|p| p.y < face_at(&suction, p.x) - 1e-9)
    {
        return Err("the faces of the edge nose cross: move its points".into());
    }
    Ok((pressure, suction))
}

/// The five control points of a Bézier edge, (s in m, n in half thicknesses), with
/// point 2 as far from the edge as makes the nose reach s = 0.
fn bezier_controls(start: [f64; 2], pull: [f64; 2], tip: f64, thickness: f64) -> [DVec2; 5] {
    let (s0, s4) = (start[0] * thickness, start[1] * thickness);
    let mut points = [
        DVec2::new(s0, 1.0),
        DVec2::new(s0 * (1.0 - pull[0]), 1.0),
        DVec2::new(0.0, tip),
        DVec2::new(s4 * (1.0 - pull[1]), -1.0),
        DVec2::new(s4, -1.0),
    ];
    let nearest = |points: &[DVec2; 5]| {
        let curve = Bezier::new(points.to_vec()).expect("five control points");
        (0..=SEARCH)
            .map(|k| curve.eval(k as f64 / SEARCH as f64).x)
            .fold(f64::INFINITY, f64::min)
    };
    // The nearest point moves toward the edge with point 2: bisect for zero.
    let reach = s0.max(s4);
    let (mut lo, mut hi) = (-4.0 * reach, reach);
    for _ in 0..48 {
        let mid = 0.5 * (lo + hi);
        points[2].x = mid;
        if nearest(&points) < 0.0 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    points[2].x = hi;
    points
}

impl EdgeProfile {
    /// The profile of `shape` on an edge `thickness` thick, m.
    pub fn new(shape: EdgeShape, thickness: f64) -> Result<Self, String> {
        shape.validate()?;
        let e = thickness;
        Ok(match shape.resolved() {
            EdgeShape::Cut => Self {
                length: 0.0,
                kind: Kind::Blunt,
            },
            EdgeShape::Ellipse { ratio } => Self {
                length: ratio * e / 2.0,
                kind: Kind::Ellipse,
            },
            EdgeShape::Taper {
                length,
                nose,
                ratio,
                asymmetry,
            } => {
                let (l, r, q) = (length * e, nose * e, 2.0 * nose / ratio);
                let slope = (1.0 - q) / (l - r);
                // A rational quadratic from the tip to the start of the taper: vertical
                // at the tip, along the taper at its end; with w = √½ and a parallel
                // taper a quarter ellipse.
                let middle = q - slope * r;
                if middle <= 0.0 {
                    return Err(
                        "the edge nose is too short to round off its taper: lengthen the nose \
                         or the taper"
                            .into(),
                    );
                }
                let w = std::f64::consts::FRAC_1_SQRT_2;
                let (a, b, c) = (DVec2::ZERO, DVec2::new(0.0, middle), DVec2::new(r, q));
                let mut half: Vec<DVec2> = (0..=SAMPLES)
                    .map(|k| {
                        let t = k as f64 / SAMPLES as f64;
                        let (b0, b1, b2) = ((1.0 - t) * (1.0 - t), 2.0 * w * t * (1.0 - t), t * t);
                        (a * b0 + b * b1 + c * b2) / (b0 + b1 + b2)
                    })
                    .collect();
                half.push(DVec2::new(l, 1.0));
                let shift = |s: f64| asymmetry * q * (1.0 - s / l).max(0.0);
                let face = |side: f64| -> Vec<DVec2> {
                    half.iter()
                        .map(|p| DVec2::new(p.x, shift(p.x) + side * p.y))
                        .collect()
                };
                Self {
                    length: l,
                    kind: Kind::Faces {
                        pressure: face(1.0),
                        suction: face(-1.0),
                    },
                }
            }
            EdgeShape::Bezier {
                start, pull, tip, ..
            } => {
                let controls = bezier_controls(start, pull, tip, e);
                let curve = Bezier::new(controls.to_vec()).expect("five control points");
                let (pressure, suction) = split_faces(&curve.sample(SAMPLES + 1))?;
                Self {
                    length: (start[0].max(start[1])) * e,
                    kind: Kind::Faces { pressure, suction },
                }
            }
        })
    }

    /// Length of the shaped part along the mean line, m.
    pub fn length(&self) -> f64 {
        self.length
    }

    /// Whether the edge is blunt.
    pub fn is_blunt(&self) -> bool {
        self.kind == Kind::Blunt
    }

    /// (pressure, suction) offsets from the mean line at `s` from the edge, in half
    /// thicknesses: (1, −1) beyond the shape.
    pub fn at(&self, s: f64) -> (f64, f64) {
        if s >= self.length {
            return (1.0, -1.0);
        }
        match &self.kind {
            Kind::Blunt => (1.0, -1.0),
            Kind::Ellipse => {
                let x = (self.length - s.max(0.0)) / self.length;
                let n = (1.0 - x * x).max(0.0).sqrt();
                (n, -n)
            }
            Kind::Faces { pressure, suction } => {
                (face_at(pressure, s.max(0.0)), face_at(suction, s.max(0.0)))
            }
        }
    }

    /// The thickness at `s` from the edge as a fraction of the full one.
    pub fn thickness_factor(&self, s: f64) -> f64 {
        let (p, q) = self.at(s);
        0.5 * (p - q)
    }

    /// Both faces from the tip to the end of the shape, (s, n) in m and half
    /// thicknesses: [pressure, suction]. A blunt edge gives the edge line.
    pub fn faces(&self) -> [Vec<DVec2>; 2] {
        match &self.kind {
            Kind::Blunt => [
                vec![DVec2::new(0.0, 0.0), DVec2::new(0.0, 1.0)],
                vec![DVec2::new(0.0, 0.0), DVec2::new(0.0, -1.0)],
            ],
            Kind::Ellipse => {
                let face = |side: f64| -> Vec<DVec2> {
                    (0..=200)
                        .map(|k| {
                            let phi = std::f64::consts::FRAC_PI_2 * k as f64 / 200.0;
                            DVec2::new(self.length * (1.0 - phi.cos()), side * phi.sin())
                        })
                        .collect()
                };
                [face(1.0), face(-1.0)]
            }
            Kind::Faces { pressure, suction } => [pressure.clone(), suction.clone()],
        }
    }
}

/// The control points of a Bézier edge `shape` on an edge `thickness` thick, (s in m,
/// n in half thicknesses); `None` for other shapes.
pub fn bezier_points(shape: EdgeShape, thickness: f64) -> Option<[DVec2; 5]> {
    match shape.resolved() {
        EdgeShape::Bezier {
            start, pull, tip, ..
        } => Some(bezier_controls(start, pull, tip, thickness)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn an_ellipse_closes_on_the_mean_line() {
        let p = EdgeProfile::new(EdgeShape::Ellipse { ratio: 4.0 }, 0.004).unwrap();
        assert_relative_eq!(p.length(), 0.008);
        assert_eq!(p.at(0.0), (0.0, -0.0));
        assert_eq!(p.at(0.01), (1.0, -1.0));
        // Half way along the nose: √(1 − ½²).
        assert_relative_eq!(p.thickness_factor(0.004), 0.75f64.sqrt());
    }

    #[test]
    fn a_taper_runs_into_a_tangent_nose() {
        let shape = EdgeShape::TAPER;
        let e = 0.004;
        let p = EdgeProfile::new(shape, e).unwrap();
        assert_relative_eq!(p.length(), 3.0 * e);
        // The tip closes, the nose is 2·0.6/2 = 0.6 of the half thickness wide where it
        // meets the taper, and the taper is straight up to full thickness.
        let (tip_p, tip_s) = p.at(0.0);
        assert!(tip_p.abs() < 1e-9 && tip_s.abs() < 1e-9);
        assert_relative_eq!(p.at(0.6 * e).0, 0.6, epsilon = 1e-6);
        let mid = 0.5 * (0.6 + 3.0) * e;
        assert_relative_eq!(p.at(mid).0, 0.8, epsilon = 1e-6);
        // Tangent where nose and taper meet: the slope just before matches the taper.
        let slope = (1.0 - 0.6) / (2.4 * e);
        let h = 1e-4 * e;
        let before = (p.at(0.6 * e).0 - p.at(0.6 * e - h).0) / h;
        assert_relative_eq!(before, slope, max_relative = 0.01);
    }

    #[test]
    fn a_parallel_taper_has_an_elliptic_nose() {
        // With the nose as wide as the blade the taper is gone, and the nose is the
        // quarter ellipse of the ratio.
        let e = 0.004;
        let p = EdgeProfile::new(
            EdgeShape::Taper {
                length: 2.0,
                nose: 0.999 * 1.0,
                ratio: 2.0,
                asymmetry: 0.0,
            },
            e,
        )
        .unwrap();
        let ellipse = EdgeProfile::new(EdgeShape::Ellipse { ratio: 2.0 }, e).unwrap();
        for k in 1..10 {
            let s = k as f64 * 0.1 * e;
            assert_relative_eq!(p.at(s).0, ellipse.at(s).0, epsilon = 2e-3);
        }
    }

    #[test]
    fn asymmetry_moves_the_tip_toward_a_face() {
        let e = 0.004;
        let shape = EdgeShape::Taper {
            length: 3.0,
            nose: 0.6,
            ratio: 2.0,
            asymmetry: 0.5,
        };
        let p = EdgeProfile::new(shape, e).unwrap();
        let (pressure, suction) = p.at(0.0);
        assert_relative_eq!(pressure, 0.3, epsilon = 1e-9);
        assert_relative_eq!(suction, 0.3, epsilon = 1e-9);
        // The thickness is the symmetric one.
        let symmetric = EdgeProfile::new(EdgeShape::TAPER, e).unwrap();
        for k in 0..30 {
            let s = k as f64 * 0.1 * e;
            assert_relative_eq!(
                p.thickness_factor(s),
                symmetric.thickness_factor(s),
                epsilon = 1e-9
            );
        }
    }

    #[test]
    fn a_bezier_nose_reaches_the_edge_and_meets_the_faces() {
        let e = 0.004;
        let p = EdgeProfile::new(EdgeShape::BEZIER, e).unwrap();
        let [pressure, suction] = p.faces();
        // The tip sits on the edge and, symmetric, on the mean line.
        assert!(pressure[0].x.abs() < 1e-6 * e);
        assert!(pressure[0].y.abs() < 1e-9);
        assert_eq!(pressure[0], suction[0]);
        // The faces end on the blade faces, two thicknesses from the edge.
        assert_relative_eq!(pressure.last().unwrap().x, 2.0 * e);
        assert_relative_eq!(pressure.last().unwrap().y, 1.0);
        assert_relative_eq!(suction.last().unwrap().y, -1.0);
        let c = bezier_points(EdgeShape::BEZIER, e).unwrap();
        assert!(c[2].x < 0.0, "point 2 lies ahead of the edge: {c:?}");
        // A tip off the mean line leans the nose.
        let lean = EdgeShape::Bezier {
            start: [2.0, 1.5],
            pull: [0.6, 0.8],
            tip: 0.4,
            symmetric: false,
        };
        let p = EdgeProfile::new(lean, e).unwrap();
        let [pressure, _] = p.faces();
        assert!(pressure[0].y > 0.0);
        assert_relative_eq!(p.length(), 2.0 * e);
        assert_eq!(p.at(1.6 * e).1, -1.0);
    }

    #[test]
    fn a_nose_that_folds_or_crosses_is_an_error() {
        let p = |x: f64, y: f64| DVec2::new(x, y);
        // Back past the edge and out again on the pressure face.
        let folded = [
            p(1.0, 1.0),
            p(0.2, 0.8),
            p(0.5, 0.5),
            p(0.0, 0.0),
            p(1.0, -1.0),
        ];
        assert!(split_faces(&folded).unwrap_err().contains("folds"));
        // The pressure face below the suction face.
        let crossed = [p(1.0, -0.5), p(0.0, 0.0), p(0.5, 0.2), p(1.0, 0.4)];
        assert!(split_faces(&crossed).unwrap_err().contains("cross"));
    }

    #[test]
    fn shapes_blend_by_their_parameters() {
        let a = EdgeShape::Ellipse { ratio: 2.0 };
        let b = EdgeShape::Ellipse { ratio: 6.0 };
        assert_eq!(a.lerp(b, 0.5), Some(EdgeShape::Ellipse { ratio: 4.0 }));
        assert_eq!(a.lerp(EdgeShape::Cut, 0.5), None);
        let blend = EdgeShape::BEZIER
            .lerp(
                EdgeShape::Bezier {
                    start: [3.0, 1.0],
                    pull: [0.6, 0.6],
                    tip: 0.2,
                    symmetric: false,
                },
                0.5,
            )
            .unwrap();
        assert_eq!(
            blend,
            EdgeShape::Bezier {
                start: [2.5, 1.5],
                pull: [0.6, 0.6],
                tip: 0.1,
                symmetric: false,
            }
        );
    }

    #[test]
    fn bad_parameters_are_named() {
        let too_wide = EdgeShape::Taper {
            length: 3.0,
            nose: 0.6,
            ratio: 1.0,
            asymmetry: 0.0,
        };
        assert!(too_wide.validate().unwrap_err().contains("ratio"));
        let long_nose = EdgeShape::Taper {
            length: 1.0,
            nose: 1.0,
            ratio: 4.0,
            asymmetry: 0.0,
        };
        assert!(long_nose.validate().is_err());
        assert!(EdgeShape::Ellipse { ratio: -1.0 }.validate().is_err());
    }

    #[test]
    fn every_nose_has_a_pressure_coefficient() {
        assert_relative_eq!(
            EdgeShape::BEZIER.min_pressure_coefficient().unwrap(),
            EdgeShape::Ellipse { ratio: 4.0 }
                .min_pressure_coefficient()
                .unwrap()
        );
        assert!(EdgeShape::TAPER.min_pressure_coefficient().unwrap() < 0.3);
    }
}
