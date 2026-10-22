//! Two-dimensional potential flow in the meridional section, between hub and shroud.
//!
//! Without swirl effects and without friction the meridional flow of a body of
//! revolution has a Stokes stream function ψ with
//!
//! ∂/∂z((1/r)·∂ψ/∂z) + ∂/∂r((1/r)·∂ψ/∂r) = 0,
//!
//! c_z = (1/r)·∂ψ/∂r and c_r = −(1/r)·∂ψ/∂z, and the flow between two stream surfaces
//! is 2π times the difference of ψ. The equation is solved with linear triangles on a
//! structured mesh: ψ = 0 on the hub, ψ = Q/2π on the shroud, and flow normal to the
//! inlet and outlet lines. For an impeller, a straight annulus ahead of the eye and a
//! ring after the outlet, along the walls' outflow direction, keep those boundaries
//! away from the blades. Unlike the
//! quasi-one-dimensional flow of the meridional step, the meridional velocity varies
//! across the channel: it is higher on the inside of a bend (see
//! `docs/theory/prestaciones.md`).

use std::f64::consts::PI;

use serde::Serialize;
use tp_math::DVec2;
use tp_math::sparse::{Triplets, conjugate_gradient};

use super::meridian::{MeridionalContour, SpanLine};
use crate::DesignError;

/// Mesh density of the solution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PotentialMesh {
    /// Lines across the channel, from inlet to outlet.
    pub stations: usize,
    /// Nodes on each line, from hub to shroud.
    pub across: usize,
}

impl Default for PotentialMesh {
    fn default() -> Self {
        Self {
            stations: 81,
            across: 21,
        }
    }
}

/// Lines across the straight extensions ahead of and after an impeller channel.
const EXTENSION_STATIONS: usize = 12;

/// The solution on the mesh.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PotentialFlow {
    pub stations: usize,
    pub across: usize,
    /// First and last station of the channel proper, without the extensions.
    pub channel: (usize, usize),
    /// (z, r) of each node, station by station from hub to shroud: index
    /// `station·across + j`.
    pub nodes: Vec<DVec2>,
    /// Stream function as a fraction of the flow: 0 on the hub, 1 on the shroud.
    pub psi: Vec<f64>,
    /// Meridional velocity at each node, m/s.
    pub cm: Vec<f64>,
    /// Conjugate gradient iterations used.
    pub iterations: usize,
}

impl PotentialFlow {
    fn index(&self, station: usize, j: usize) -> usize {
        station * self.across + j
    }

    /// The stream surface enclosing `fraction` of the flow from the hub, through the
    /// channel proper, with the meridional velocity along it.
    pub fn streamline(&self, fraction: f64) -> SpanLine {
        let (points, cm) = (self.channel.0..=self.channel.1)
            .map(|i| {
                let psi = |j| self.psi[self.index(i, j)];
                let node = |j| self.nodes[self.index(i, j)];
                let j = (0..self.across - 1)
                    .find(|&j| psi(j + 1) >= fraction)
                    .unwrap_or(self.across - 2);
                let span = psi(j + 1) - psi(j);
                let t = if span.abs() > f64::EPSILON {
                    ((fraction - psi(j)) / span).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let cm = |j| self.cm[self.index(i, j)];
                (
                    node(j) + (node(j + 1) - node(j)) * t,
                    cm(j) + (cm(j + 1) - cm(j)) * t,
                )
            })
            .unzip();
        SpanLine {
            fraction,
            points,
            cm,
        }
    }

    /// Meridional velocity along a wall (`j` = 0 hub, `across − 1` shroud) against
    /// the fraction of its length from the inlet.
    fn along_wall(&self, j: usize) -> Vec<[f64; 2]> {
        let (first, last) = self.channel;
        let mut length = vec![0.0; last - first + 1];
        for i in first + 1..=last {
            let a = self.nodes[self.index(i - 1, j)];
            let b = self.nodes[self.index(i, j)];
            length[i - first] = length[i - first - 1] + a.distance(b);
        }
        let total = length[last - first].max(f64::EPSILON);
        (first..=last)
            .map(|i| [length[i - first] / total, self.cm[self.index(i, j)]])
            .collect()
    }

    /// Meridional velocity along the hub, against the fraction of its length.
    pub fn hub_velocity(&self) -> Vec<[f64; 2]> {
        self.along_wall(0)
    }

    /// Meridional velocity along the shroud, against the fraction of its length.
    pub fn shroud_velocity(&self) -> Vec<[f64; 2]> {
        self.along_wall(self.across - 1)
    }
}

/// Solves the potential flow `flow` (m³/s) through the channel between the walls
/// `hub(t)` and `shroud(t)`, both from the inlet (t = 0) to the outlet (t = 1).
pub fn solve_channel(
    hub: impl Fn(f64) -> DVec2,
    shroud: impl Fn(f64) -> DVec2,
    flow: f64,
    mesh: PotentialMesh,
) -> Result<PotentialFlow, DesignError> {
    let ns = mesh.stations.max(3);
    let lines: Vec<(DVec2, DVec2)> = (0..ns)
        .map(|i| {
            let t = i as f64 / (ns - 1) as f64;
            (hub(t), shroud(t))
        })
        .collect();
    solve_lines(&lines, (0, ns - 1), flow, mesh.across)
}

/// Solves the potential flow through the channel whose cross lines run from the hub
/// point to the shroud point of each entry of `lines`; `channel` marks the stations
/// that belong to the channel proper.
fn solve_lines(
    lines: &[(DVec2, DVec2)],
    channel: (usize, usize),
    flow: f64,
    across: usize,
) -> Result<PotentialFlow, DesignError> {
    let (ns, nj) = (lines.len(), across.max(3));
    let index = |i: usize, j: usize| i * nj + j;
    let mut nodes = Vec::with_capacity(ns * nj);
    for &(h, s) in lines {
        for j in 0..nj {
            let lambda = j as f64 / (nj - 1) as f64;
            nodes.push(h + (s - h) * lambda);
        }
    }
    if nodes.iter().any(|p| p.y <= 0.0) {
        return Err(DesignError::Infeasible(
            "the meridional channel must lie off the axis (r > 0)".into(),
        ));
    }

    // Two triangles per cell.
    let mut triangles = Vec::with_capacity(2 * (ns - 1) * (nj - 1));
    for i in 0..ns - 1 {
        for j in 0..nj - 1 {
            let (a, b, c, d) = (
                index(i, j),
                index(i + 1, j),
                index(i + 1, j + 1),
                index(i, j + 1),
            );
            triangles.push([a, b, c]);
            triangles.push([a, c, d]);
        }
    }
    // Shape-function gradients, area and mean radius of each triangle.
    let elements: Vec<([DVec2; 3], f64, f64)> = triangles
        .iter()
        .map(|t| {
            let p = t.map(|k| nodes[k]);
            let twice_area = (p[1] - p[0]).perp_dot(p[2] - p[0]);
            let gradients = [0, 1, 2].map(|a| {
                let (b, c) = ((a + 1) % 3, (a + 2) % 3);
                (p[c] - p[b]).perp() / twice_area
            });
            let radius = (p[0].y + p[1].y + p[2].y) / 3.0;
            (gradients, 0.5 * twice_area.abs(), radius)
        })
        .collect();
    if elements.iter().any(|e| e.1.is_nan() || e.1 <= 0.0) {
        return Err(DesignError::Infeasible(
            "the meridional mesh has degenerate cells; check the contour".into(),
        ));
    }

    // Unknowns: the nodes inside the channel. Hub ψ = 0, shroud ψ = 1.
    let fixed = |k: usize| {
        let j = k % nj;
        if j == 0 {
            Some(0.0)
        } else if j == nj - 1 {
            Some(1.0)
        } else {
            None
        }
    };
    let mut unknown = vec![usize::MAX; nodes.len()];
    let mut count = 0;
    for (k, slot) in unknown.iter_mut().enumerate() {
        if fixed(k).is_none() {
            *slot = count;
            count += 1;
        }
    }
    let mut matrix = Triplets::new(count);
    let mut rhs = vec![0.0; count];
    for (t, (gradients, area, radius)) in triangles.iter().zip(&elements) {
        for a in 0..3 {
            let Some(row) = Some(unknown[t[a]]).filter(|&u| u != usize::MAX) else {
                continue;
            };
            for b in 0..3 {
                let k = area / radius * gradients[a].dot(gradients[b]);
                match fixed(t[b]) {
                    Some(value) => rhs[row] -= k * value,
                    None => matrix.add(row, unknown[t[b]], k),
                }
            }
        }
    }
    let matrix = matrix.build();
    // Start from ψ growing linearly across the channel.
    let mut x: Vec<f64> = (0..nodes.len())
        .filter(|&k| fixed(k).is_none())
        .map(|k| (k % nj) as f64 / (nj - 1) as f64)
        .collect();
    let iterations = conjugate_gradient(&matrix, &rhs, &mut x, 1e-10, 20 * count)
        .map_err(|e| DesignError::Infeasible(format!("potential flow: {e}")))?;
    let psi: Vec<f64> = (0..nodes.len())
        .map(|k| fixed(k).unwrap_or_else(|| x[unknown[k]]))
        .collect();

    // Velocity per triangle, then area-weighted at the nodes.
    let scale = flow / (2.0 * PI);
    let mut sum = vec![0.0; nodes.len()];
    let mut weight = vec![0.0; nodes.len()];
    for (t, (gradients, area, radius)) in triangles.iter().zip(&elements) {
        let grad: DVec2 = (0..3).map(|a| gradients[a] * psi[t[a]]).sum();
        let speed = scale * grad.length() / radius;
        for &k in t {
            sum[k] += speed * area;
            weight[k] += area;
        }
    }
    let cm = sum.iter().zip(&weight).map(|(s, w)| s / w).collect();

    Ok(PotentialFlow {
        stations: ns,
        across: nj,
        channel,
        nodes,
        psi,
        cm,
        iterations,
    })
}

/// Potential flow `flow` (m³/s) through the meridional contour of an impeller, with
/// an axial annulus as long as the eye is high ahead of it and, after it, a ring that
/// carries on each wall in the direction it leaves the outlet (radial, or inclined at
/// ε_s on a semi-axial impeller) for twice the outlet width.
pub fn solve(
    contour: &MeridionalContour,
    flow: f64,
    mesh: PotentialMesh,
) -> Result<PotentialFlow, DesignError> {
    let ns = mesh.stations.max(3);
    let (hub_in, shroud_in) = (contour.hub_at(0.0), contour.shroud_at(0.0));
    let (hub_out, shroud_out) = (contour.hub_at(1.0), contour.shroud_at(1.0));
    let upstream = (shroud_in.y - hub_in.y).abs();
    let (hub_dir, shroud_dir) = (contour.hub.end_direction(), contour.shroud.end_direction());
    // The width of the outlet across the mean outflow direction.
    let along = (hub_dir + shroud_dir).normalize();
    let downstream = 2.0 * (shroud_out - hub_out).perp_dot(along).abs();
    let ne = EXTENSION_STATIONS;
    let mut lines = Vec::with_capacity(ns + 2 * ne);
    for k in 0..ne {
        let back = upstream * (ne - k) as f64 / ne as f64;
        lines.push((hub_in - DVec2::X * back, shroud_in - DVec2::X * back));
    }
    for i in 0..ns {
        let t = i as f64 / (ns - 1) as f64;
        lines.push((contour.hub_at(t), contour.shroud_at(t)));
    }
    for k in 1..=ne {
        let out = downstream * k as f64 / ne as f64;
        lines.push((hub_out + hub_dir * out, shroud_out + shroud_dir * out));
    }
    solve_lines(&lines, (ne, ne + ns - 1), flow, mesh.across)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::impeller::tests::reference_geometry;
    use approx::assert_relative_eq;

    #[test]
    fn an_annulus_has_uniform_axial_velocity() {
        let (rh, rs, q) = (0.05, 0.1, 0.03);
        let flow = solve_channel(
            |t| DVec2::new(0.3 * t, rh),
            |t| DVec2::new(0.3 * t, rs),
            q,
            PotentialMesh {
                stations: 21,
                across: 41,
            },
        )
        .unwrap();
        let exact = q / (PI * (rs * rs - rh * rh));
        // Away from the walls, where the node average is two-sided.
        for i in 2..flow.stations - 2 {
            for j in 2..flow.across - 2 {
                let cm = flow.cm[i * flow.across + j];
                assert_relative_eq!(cm, exact, max_relative = 0.01);
            }
        }
        // ψ follows the swept area: (r² − r_h²)/(r_s² − r_h²).
        let k = 10 * flow.across + 20;
        let r = flow.nodes[k].y;
        assert_relative_eq!(
            flow.psi[k],
            (r * r - rh * rh) / (rs * rs - rh * rh),
            epsilon = 1e-3
        );
    }

    #[test]
    fn between_parallel_disks_the_velocity_falls_as_one_over_r() {
        let (b, q) = (0.02, 0.01);
        let flow = solve_channel(
            |t| DVec2::new(0.0, 0.1 + 0.2 * t),
            |t| DVec2::new(-b, 0.1 + 0.2 * t),
            q,
            PotentialMesh {
                stations: 41,
                across: 11,
            },
        )
        .unwrap();
        for i in 2..flow.stations - 2 {
            let k = i * flow.across + 5;
            let r = flow.nodes[k].y;
            assert_relative_eq!(flow.cm[k], q / (2.0 * PI * r * b), max_relative = 0.01);
        }
    }

    #[test]
    fn the_impeller_bend_speeds_up_the_shroud() {
        let g = reference_geometry();
        let flow = solve(&g.contour, g.basis.flow, PotentialMesh::default()).unwrap();
        assert!(flow.iterations > 0);
        // ψ rises from hub to shroud on every station.
        for i in 0..flow.stations {
            for j in 1..flow.across {
                let k = i * flow.across + j;
                assert!(flow.psi[k] >= flow.psi[k - 1] - 1e-9);
            }
        }
        // In the bend from axial to radial the shroud is the inner wall.
        let hub = flow.hub_velocity();
        let shroud = flow.shroud_velocity();
        let middle = hub.len() / 3;
        assert!(
            shroud[middle][1] > hub[middle][1],
            "{:?}",
            (hub[middle], shroud[middle])
        );
        // The mean streamline sits inside the channel and ends at the outlet radius.
        let mean = flow.streamline(0.5);
        let r2 = g.basis.outlet_diameter / 2.0;
        assert_relative_eq!(mean.points.last().unwrap().y, r2, max_relative = 1e-9);
        // The average speed at the outlet matches continuity.
        let outlet = (0..flow.across)
            .map(|j| flow.cm[flow.channel.1 * flow.across + j])
            .sum::<f64>()
            / flow.across as f64;
        let continuity = g.basis.flow / (PI * g.basis.outlet_diameter * g.basis.outlet_width);
        assert_relative_eq!(outlet, continuity, max_relative = 0.05);
    }

    #[test]
    fn a_semi_axial_outlet_leaves_along_its_walls() {
        let g = crate::impeller::tests::semi_axial_geometry();
        let flow = solve(&g.contour, g.basis.flow, PotentialMesh::default()).unwrap();
        // The ring after the outlet carries on in the walls' outflow direction, so the
        // flow leaves the inclined outlet without piling up on the shroud.
        let station = flow.channel.1;
        let (hub, shroud) = (
            flow.cm[station * flow.across],
            flow.cm[station * flow.across + flow.across - 1],
        );
        assert!(
            shroud / hub < 1.6,
            "outlet c_m hub {hub:.2}, shroud {shroud:.2}"
        );
        // Mean outlet velocity: the flow over the area π·(r_h + r_s)·b across the walls.
        let (h, s) = (g.contour.hub.end(), g.contour.shroud.end());
        let along = (g.contour.hub.end_direction() + g.contour.shroud.end_direction()).normalize();
        let width = (s - h).perp_dot(along).abs();
        let continuity = g.basis.flow / (PI * (h.y + s.y) * width);
        let outlet = (0..flow.across)
            .map(|j| flow.cm[station * flow.across + j])
            .sum::<f64>()
            / flow.across as f64;
        assert_relative_eq!(outlet, continuity, max_relative = 0.1);
    }
}
