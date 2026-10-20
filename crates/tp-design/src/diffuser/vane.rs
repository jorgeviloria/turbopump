//! Plan-view geometry of the vanes of a radial diffuser: mean line by conformal mapping
//! in the plane (dθ = dr/(r·tan α)), thickness from leading to trailing edge, and the
//! throat between neighbouring vanes.
//!
//! The geometry is stored for an impeller turning in the positive sense about +z, with
//! the leading edge of the first vane on the +x axis; [`VaneGeometry::outline`] turns it
//! to any vane and mirrors it for the other sense of rotation.

use std::f64::consts::{PI, TAU};

use tp_math::DVec2;

use crate::DesignError;
use crate::impeller::blade::{EdgeShape, edge_factor};

/// What a vane is built from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VaneShape {
    pub count: u32,
    pub inlet_radius: f64,
    pub outlet_radius: f64,
    /// α₃B from the circumferential direction, rad.
    pub inlet_angle: f64,
    /// α₄B, rad.
    pub outlet_angle: f64,
    /// Exponent p of the angle law α = α₃B + (α₄B − α₃B)·t^p, t = (r − r₃)/(r₄ − r₃).
    pub angle_exponent: f64,
    pub leading_thickness: f64,
    pub trailing_thickness: f64,
    pub leading_edge: EdgeShape,
    pub trailing_edge: EdgeShape,
    pub points: usize,
}

/// One vane in plan view and the channel it forms with the next one.
#[derive(Debug, Clone, PartialEq)]
pub struct VaneGeometry {
    pub count: u32,
    /// Mean line from leading to trailing edge.
    pub mean_line: Vec<DVec2>,
    /// Side facing the axis.
    pub inner_side: Vec<DVec2>,
    /// Side facing away from the axis.
    pub outer_side: Vec<DVec2>,
    /// Angle the vane wraps around the axis, rad.
    pub wrap_angle: f64,
    /// Shortest distance from the leading edge of the next vane to this one, m.
    pub throat: f64,
    /// Radius where the throat meets this vane, m.
    pub throat_radius: f64,
    /// Length of this vane from the throat to the trailing edge, m.
    pub channel_length: f64,
}

/// Fine steps for integrating the conformal mapping between two stations.
const SUBSTEPS: usize = 20;

impl VaneGeometry {
    pub fn build(shape: &VaneShape) -> Result<Self, DesignError> {
        let (r3, r4) = (shape.inlet_radius, shape.outlet_radius);
        let n = shape.points.max(3);
        let angle = |r: f64| {
            let t = ((r - r3) / (r4 - r3)).clamp(0.0, 1.0);
            shape.inlet_angle
                + (shape.outlet_angle - shape.inlet_angle) * t.powf(shape.angle_exponent)
        };
        // Stations clustered toward both edges.
        let radii: Vec<f64> = (0..n)
            .map(|i| {
                let t = i as f64 / (n - 1) as f64;
                r3 + (r4 - r3) * 0.5 * (1.0 - (PI * t).cos())
            })
            .collect();
        let mut theta = vec![0.0; n];
        for i in 1..n {
            let (a, b) = (radii[i - 1], radii[i]);
            let h = (b - a) / SUBSTEPS as f64;
            theta[i] = theta[i - 1]
                + (0..SUBSTEPS)
                    .map(|k| {
                        let r = a + (k as f64 + 0.5) * h;
                        h / (r * angle(r).tan())
                    })
                    .sum::<f64>();
        }
        let mean_line: Vec<DVec2> = radii
            .iter()
            .zip(&theta)
            .map(|(&r, &t)| DVec2::new(r * t.cos(), r * t.sin()))
            .collect();

        let mut lengths = vec![0.0; n];
        for i in 1..n {
            lengths[i] = lengths[i - 1] + mean_line[i].distance(mean_line[i - 1]);
        }
        let total = lengths[n - 1];
        let (e3, e4) = (shape.leading_thickness, shape.trailing_thickness);
        let mut inner_side = Vec::with_capacity(n);
        let mut outer_side = Vec::with_capacity(n);
        for i in 0..n {
            let s = lengths[i];
            let e = (e3 + (e4 - e3) * s / total)
                * edge_factor(shape.leading_edge, e3, s)
                * edge_factor(shape.trailing_edge, e4, total - s);
            let tangent =
                (mean_line[(i + 1).min(n - 1)] - mean_line[i.saturating_sub(1)]).normalize();
            // Left of the direction of travel: toward the axis.
            let normal = DVec2::new(-tangent.y, tangent.x);
            inner_side.push(mean_line[i] + normal * (e / 2.0));
            outer_side.push(mean_line[i] - normal * (e / 2.0));
        }

        // The next vane starts one pitch further round; its leading edge faces the inner
        // side of this vane.
        let pitch = TAU / f64::from(shape.count);
        let next_le = DVec2::new(r3 * pitch.cos(), r3 * pitch.sin());
        let (index, throat) = inner_side
            .iter()
            .enumerate()
            .map(|(i, p)| (i, p.distance(next_le)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .ok_or_else(|| DesignError::Infeasible("the vane has no points".into()))?;
        let throat_radius = inner_side[index].length();
        let channel_length = total - lengths[index];
        Ok(Self {
            count: shape.count,
            wrap_angle: theta[n - 1],
            mean_line,
            inner_side,
            outer_side,
            throat,
            throat_radius,
            channel_length,
        })
    }

    /// Outer and inner side of vane `index`, from leading to trailing edge, for
    /// rotation `sign`.
    pub fn sides(&self, index: u32, sign: f64) -> (Vec<DVec2>, Vec<DVec2>) {
        let angle = TAU * f64::from(index) / f64::from(self.count);
        let (s, c) = angle.sin_cos();
        let place = |p: &DVec2| {
            let q = DVec2::new(c * p.x - s * p.y, s * p.x + c * p.y);
            DVec2::new(q.x, sign * q.y)
        };
        (
            self.outer_side.iter().map(place).collect(),
            self.inner_side.iter().map(place).collect(),
        )
    }

    /// Closed outline of vane `index` (leading edge along the outer side, back along
    /// the inner side) for rotation `sign`.
    pub fn outline(&self, index: u32, sign: f64) -> Vec<DVec2> {
        let (outer, inner) = self.sides(index, sign);
        outer.into_iter().chain(inner.into_iter().rev()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    fn shape(inlet_angle: f64, outlet_angle: f64) -> VaneShape {
        VaneShape {
            count: 12,
            inlet_radius: 0.0873,
            outlet_radius: 0.1255,
            inlet_angle,
            outlet_angle,
            angle_exponent: 1.0,
            leading_thickness: 0.002,
            trailing_thickness: 0.003,
            leading_edge: EdgeShape::Ellipse { ratio: 3.0 },
            trailing_edge: EdgeShape::Cut,
            points: 61,
        }
    }

    #[test]
    fn constant_angle_is_a_logarithmic_spiral() {
        let a = 12f64.to_radians();
        let g = VaneGeometry::build(&shape(a, a)).unwrap();
        // θ = ln(r₄/r₃)/tan α.
        assert_relative_eq!(
            g.wrap_angle,
            (0.1255f64 / 0.0873).ln() / a.tan(),
            max_relative = 1e-6
        );
        let le = g.mean_line[0];
        assert_relative_eq!(le.x, 0.0873, epsilon = 1e-12);
        assert_relative_eq!(le.y, 0.0, epsilon = 1e-12);
    }

    #[test]
    fn throat_lies_between_neighbouring_vanes() {
        let g = VaneGeometry::build(&shape(10f64.to_radians(), 25f64.to_radians())).unwrap();
        let pitch = TAU * 0.0873 / 12.0;
        // Narrower than the pitch, wider than nothing.
        assert!(g.throat > 0.0 && g.throat < pitch);
        assert!(g.throat_radius > 0.0873 && g.throat_radius < 0.1255);
        assert!(g.channel_length > 0.0);
        // Thickness: zero at the elliptic leading edge, full at the blunt trailing edge.
        let n = g.mean_line.len();
        assert!(g.inner_side[0].distance(g.outer_side[0]) < 1e-9);
        assert_relative_eq!(
            g.inner_side[n - 1].distance(g.outer_side[n - 1]),
            0.003,
            max_relative = 1e-9
        );
        // The inner side is the one nearer the axis.
        assert!(g.inner_side[n / 2].length() < g.outer_side[n / 2].length());
    }

    #[test]
    fn outlines_turn_and_mirror() {
        let g = VaneGeometry::build(&shape(10f64.to_radians(), 20f64.to_radians())).unwrap();
        let first = g.outline(0, 1.0);
        let second = g.outline(1, 1.0);
        let mirrored = g.outline(0, -1.0);
        let pitch = TAU / 12.0;
        let a = first[5].y.atan2(first[5].x);
        let b = second[5].y.atan2(second[5].x);
        assert_relative_eq!(b - a, pitch, epsilon = 1e-12);
        assert_relative_eq!(mirrored[5].y, -first[5].y);
    }
}
