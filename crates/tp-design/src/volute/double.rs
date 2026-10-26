//! The sections of a double volute whose cross sections are not straight-walled
//! (radius based, line segments, Bézier).
//!
//! The rib is a vertical wall at one radius across the whole section `W` of the channel.
//! Straight walls make the inner volute, the rib and the outer channel closed forms of
//! their heights (the trapezoid or rectangle grown to each); any other shape is not
//! self-similar, so the inner volute is the part of `W` below the rib, and the outer
//! channel the part beyond it, and the two have to be sized together: at each wrap angle
//! the height `a` of the rib's inner face and the height `H` of `W` are the pair for
//! which
//!
//! - the inner part has the measure its law asks for (the angular momentum of the flow
//!   of its wrap, Gülich Chap. 7.8.1, item 8, or the area for a velocity rule), and
//! - the outer part has the area the outer channel is to have there.
//!
//! The pairs are solved on a table of wrap angles and interpolated between them.

use std::f64::consts::TAU;

use tp_math::roots;

use super::section::{self, CrossSections, Measure};
use super::{AreaLaw, DesignError, Rib, SectionSize, Shape, SpiralDevelopment};

/// Nodes across the height of `W` for the integrals of its width.
const NODES: usize = 240;
/// Wrap angles of the table, from the rib nose to the throat.
const ROWS: usize = 48;

/// The sizes of a double volute at one wrap angle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DoubleRow {
    pub angle: f64,
    /// Height of the inner face of the rib above r'_z, m.
    pub inner_height: f64,
    /// Height of the whole section above r'_z, m.
    pub whole_height: f64,
    /// Area of the inner volute below the rib and radius of its centroid, m² and m.
    pub inner_area: f64,
    pub inner_centroid: f64,
}

/// The table of a double volute of free sections.
#[derive(Debug, Clone, PartialEq)]
pub struct FreeDouble {
    rows: Vec<DoubleRow>,
    /// Throat correction of the inner volute (eq. 7.28), m.
    pub correction: f64,
}

/// Integrals of the width of one section `W` over its height, on `NODES` + 1 nodes.
struct Profile {
    h: Vec<f64>,
    area: Vec<f64>,
    /// ∫ r·w dh, m³.
    moment: Vec<f64>,
    measure: Vec<f64>,
}

impl Profile {
    fn new(sections: &CrossSections, height: f64, measure: Measure) -> Self {
        let rz = sections.stagnation_radius;
        let outline = sections.outline(TAU, height);
        let h: Vec<f64> = (0..=NODES)
            .map(|k| height * k as f64 / NODES as f64)
            .collect();
        // Just inside the ends the width of a section with a pointed top is 0, as it is.
        let w: Vec<f64> = h
            .iter()
            .map(|&x| section::width_at(&outline, x.clamp(1e-9 * height, (1.0 - 1e-9) * height)))
            .collect();
        let weight = |x: f64| -> f64 {
            match measure {
                Measure::Swirl {
                    exponent,
                    reference_radius,
                } => (reference_radius / (rz + x)).powf(exponent),
                _ => 1.0,
            }
        };
        let (mut area, mut moment, mut meas) = (vec![0.0], vec![0.0], vec![0.0]);
        for k in 0..NODES {
            let dh = h[k + 1] - h[k];
            area.push(area[k] + 0.5 * (w[k] + w[k + 1]) * dh);
            moment.push(moment[k] + 0.5 * ((rz + h[k]) * w[k] + (rz + h[k + 1]) * w[k + 1]) * dh);
            meas.push(meas[k] + 0.5 * (weight(h[k]) * w[k] + weight(h[k + 1]) * w[k + 1]) * dh);
        }
        Self {
            h,
            area,
            moment,
            measure: meas,
        }
    }

    fn total_area(&self) -> f64 {
        self.area[NODES]
    }

    fn at(&self, values: &[f64], x: f64) -> f64 {
        let x = x.clamp(0.0, self.h[NODES]);
        let k = ((x / self.h[NODES]) * NODES as f64).floor() as usize;
        let k = k.min(NODES - 1);
        let w = (x - self.h[k]) / (self.h[k + 1] - self.h[k]);
        values[k] + (values[k + 1] - values[k]) * w
    }

    /// The height below which the section holds `area`.
    fn height_for_area(&self, area: f64) -> f64 {
        let k = self.area.partition_point(|a| *a < area).clamp(1, NODES);
        let w = (area - self.area[k - 1]) / (self.area[k] - self.area[k - 1]).max(1e-300);
        self.h[k - 1] + (self.h[k] - self.h[k - 1]) * w.clamp(0.0, 1.0)
    }
}

/// What the inner volute has to hold at a wrap angle.
#[derive(Clone, Copy)]
enum Target {
    Swirl(f64),
    Area(f64),
}

impl FreeDouble {
    /// Solves the table of `rib` on the sections of `shape` (a free one) by the law of
    /// `rule`, for a cutwater thickness `e3` (the throat correction of eq. 7.28 when
    /// `corrected`).
    pub(super) fn build(
        shape: &Shape,
        rule: &SpiralDevelopment,
        rib: &Rib,
        e3: f64,
        corrected: bool,
    ) -> Result<Self, DesignError> {
        let sections = shape.free.as_ref().expect("a free shape").as_ref().clone();
        let (law, start) = (rule.law, rib.start);
        let measure = match law {
            AreaLaw::Stepanoff | AreaLaw::ConstantVelocity => Measure::Area,
            AreaLaw::AngularMomentum => Measure::Swirl {
                exponent: rule.exponent,
                reference_radius: rule.reference_radius,
            },
            _ => Measure::Swirl {
                exponent: 1.0,
                reference_radius: rule.reference_radius,
            },
        };
        let flow =
            |angle: f64| rule.reference_flow * (angle - start) / TAU / rule.reference_velocity;
        // The measure of the inner volute of the full wrap: the throat by angular
        // momentum, for the rules that scale it (constant velocity) too.
        let momentum = Measure::Swirl {
            exponent: 1.0,
            reference_radius: rule.reference_radius,
        };
        let solve = |angle: f64, measure: Measure, target: Target| {
            Self::solve(&sections, rib, angle, measure, target)
        };
        // Constant velocity: the area grows linearly from nothing to that of the throat by
        // angular momentum.
        let area_end = if law == AreaLaw::ConstantVelocity {
            let (a, h) = solve(TAU, momentum, Target::Swirl(flow(TAU)))?;
            Some(Profile::new(&sections, h, Measure::Area).at_area(a))
        } else {
            None
        };
        let mut rows = Vec::with_capacity(ROWS + 1);
        for i in 0..=ROWS {
            let angle = start + (TAU - start) * i as f64 / ROWS as f64;
            if i == 0 {
                rows.push(Self::row(&sections, rib, angle, 0.0, measure, 0.0)?);
                continue;
            }
            let target = match (law, area_end) {
                (AreaLaw::ConstantVelocity, Some(end)) => {
                    Target::Area(end * (angle - start) / (TAU - start))
                }
                (AreaLaw::Stepanoff, _) => Target::Area(flow(angle)),
                _ => Target::Swirl(flow(angle)),
            };
            let (a, h) = solve(angle, measure, target)?;
            rows.push(Self::row(&sections, rib, angle, a, measure, h)?);
        }
        // The throat correction ramps the inner volute up (eq. 7.28), on the whole sections
        // the outer channel still has its area: the walls move out by the same height.
        let end = rows.last().map_or(0.0, |r| r.inner_height);
        let correction = if corrected && end > 0.0 {
            0.2 * e3 * e3 / end
        } else {
            0.0
        };
        for row in &mut rows {
            let fraction = ((row.angle - start) / (TAU - start)).clamp(0.0, 1.0);
            row.inner_height += correction * fraction;
        }
        let mut table = Self { rows, correction };
        table.refresh(&sections, rib);
        Ok(table)
    }

    /// The area and centroid of the inner volute at its (corrected) heights.
    fn refresh(&mut self, sections: &CrossSections, rib: &Rib) {
        let _ = rib;
        for row in &mut self.rows {
            if row.inner_height <= 0.0 {
                row.inner_area = 0.0;
                row.inner_centroid = sections.stagnation_radius;
                continue;
            }
            let profile = Profile::new(sections, row.whole_height, Measure::Area);
            row.inner_area = profile.at(&profile.area, row.inner_height);
            let moment = profile.at(&profile.moment, row.inner_height);
            row.inner_centroid = moment / row.inner_area;
        }
    }

    fn row(
        sections: &CrossSections,
        rib: &Rib,
        angle: f64,
        inner: f64,
        measure: Measure,
        whole: f64,
    ) -> Result<DoubleRow, DesignError> {
        let whole = if whole > 0.0 {
            whole
        } else {
            // The rib nose: the whole section is the throat of the first partial volute.
            rib.outer_throat.height
        };
        let _ = measure;
        let profile = Profile::new(sections, whole, Measure::Area);
        let area = profile.at(&profile.area, inner);
        let centroid = if inner > 0.0 {
            profile.at(&profile.moment, inner) / area
        } else {
            sections.stagnation_radius
        };
        Ok(DoubleRow {
            angle,
            inner_height: inner,
            whole_height: whole,
            inner_area: area,
            inner_centroid: centroid,
        })
    }

    /// The height of the inner volute and of the whole section at `angle`.
    fn solve(
        sections: &CrossSections,
        rib: &Rib,
        angle: f64,
        measure: Measure,
        target: Target,
    ) -> Result<(f64, f64), DesignError> {
        let t = rib.thickness;
        let outer = rib.channel_area(angle);
        let wanted = match target {
            Target::Swirl(x) | Target::Area(x) => x,
        };
        // The inner height for a whole height: the cut that leaves the outer channel its
        // area, minus the rib; its measure less the target.
        let evaluate = |h: f64| -> (f64, f64) {
            let profile = Profile::new(sections, h, measure);
            let below = profile.total_area() - outer;
            if below <= profile.at(&profile.area, t) {
                return (0.0, -wanted);
            }
            let cut = profile.height_for_area(below);
            let a = (cut - t).max(0.0);
            let have = match target {
                Target::Area(_) => profile.at(&profile.area, a),
                Target::Swirl(_) => profile.at(&profile.measure, a),
            };
            (a, have - wanted)
        };
        let residual = |h: f64| evaluate(h).1;
        // Smallest whole height: the standalone section of the outer area, plus what the
        // rib band takes, found by growing from the outer area alone.
        let mut lo = sections.height_for_area(TAU, outer)?;
        while residual(lo) > 0.0 && lo > 1e-9 {
            lo *= 0.8;
        }
        let mut hi = lo.max(1e-6);
        let mut guard = 0;
        while residual(hi) < 0.0 {
            hi *= 1.25;
            guard += 1;
            if guard > 80 {
                return Err(DesignError::Infeasible(
                    "the double volute does not close: the rib and the outer channel leave \
                     no room for the inner volute"
                        .into(),
                ));
            }
        }
        let h = roots::brent(residual, lo, hi, 1e-12 * hi)
            .map_err(|e| DesignError::Infeasible(e.to_string()))?;
        Ok((evaluate(h).0, h))
    }

    /// The sizes at `angle`, interpolated between the rows of the table.
    pub fn row_at(&self, angle: f64) -> DoubleRow {
        let rows = &self.rows;
        let (first, last) = (rows[0], rows[rows.len() - 1]);
        if angle <= first.angle {
            return first;
        }
        if angle >= last.angle {
            return last;
        }
        let k = rows
            .partition_point(|r| r.angle < angle)
            .clamp(1, rows.len() - 1);
        let (a, b) = (rows[k - 1], rows[k]);
        let w = (angle - a.angle) / (b.angle - a.angle);
        let lerp = |x: f64, y: f64| x + (y - x) * w;
        DoubleRow {
            angle,
            inner_height: lerp(a.inner_height, b.inner_height),
            whole_height: lerp(a.whole_height, b.whole_height),
            inner_area: lerp(a.inner_area, b.inner_area),
            inner_centroid: lerp(a.inner_centroid, b.inner_centroid),
        }
    }

    /// The size of the inner volute at `angle`.
    pub fn inner_size(&self, angle: f64, rz: f64) -> SectionSize {
        let row = self.row_at(angle);
        SectionSize {
            height: row.inner_height,
            outer_radius: rz + row.inner_height,
            area: row.inner_area,
            centroid_radius: row.inner_centroid,
            lift: 0.0,
        }
    }
}

impl Profile {
    fn at_area(&self, x: f64) -> f64 {
        self.at(&self.area, x)
    }
}

/// Area of the part of the section of height `total` of `sections` below the height `cut`.
pub(super) fn clip_area(sections: &CrossSections, total: f64, cut: f64) -> f64 {
    let profile = Profile::new(sections, total, Measure::Area);
    profile.at(&profile.area, cut)
}
