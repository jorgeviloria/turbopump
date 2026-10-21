//! Solids of a radial impeller and its flow domain.
//!
//! - **Channel**: the meridional region between hub and shroud revolved through 360°.
//! - **Blade**: B-spline loft through the closed blade sections (pressure side, nose,
//!   suction side, trailing edge), extended slightly beyond hub and shroud so that
//!   boolean operations never meet coincident faces.
//! - **Flow domain**: channel minus all blades.
//! - **Impeller**: hub disk and shroud (one profile of revolution less the channel) ∪
//!   (blades ∩ channel), less the balance holes.
//! - **Fillets** where the blades meet hub and shroud, on the impeller and on the flow
//!   domain alike.
//! - **Hub disk**: a wall as thick as the shroud behind the hub contour and a boss
//!   around the shaft bore, hollow between them on the rear side.
//!
//! Coordinates: z along the axis in the inflow direction, the meridional profile drawn
//! in the xz plane (x = r), all in millimetres.

use std::f64::consts::PI;

use tp_design::impeller::ImpellerGeometry;
use tp_design::impeller::blade::BladeSection;
use tp_math::{DVec2, DVec3};

use crate::CadError;
use crate::occ::{self, Shape};

/// Model units per metre.
const MM: f64 = 1000.0;
/// Below this distance (mm) two section points are taken as one (closed blade edge).
const CLOSED_EDGE: f64 = 1e-6;

/// Balance holes through the hub disk.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BalanceHoleOptions {
    pub count: u32,
    /// m
    pub diameter: f64,
    /// Diameter of the annular seal on the rear shroud d_s2, m: the holes lie inside it.
    pub seal_diameter: f64,
}

/// The balance holes drawn: `count` holes of `diameter` on `pitch_diameter`, m.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BalanceHoles {
    pub count: u32,
    pub diameter: f64,
    pub pitch_diameter: f64,
}

/// Options of the CAD model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CadOptions {
    /// Wall thickness of hub disk and shroud, m. `None`: max(3 mm, 0.02·d₂).
    pub wall_thickness: Option<f64>,
    /// Bore diameter of the hub, m. `None`: the hub diameter at the eye.
    pub bore_diameter: Option<f64>,
    /// Outer diameter of the boss around the bore, m. `None`: the bore plus a wall of
    /// a quarter of the bore, and no less than the wall thickness.
    pub boss_diameter: Option<f64>,
    /// Fillet radius where the blades meet hub and shroud, m. `None`: half the blade
    /// thickness; zero leaves the edges sharp.
    pub fillet_radius: Option<f64>,
    pub balance_holes: Option<BalanceHoleOptions>,
    /// Points used to interpolate hub and shroud contours.
    pub contour_points: usize,
    /// Extension of the blade beyond hub and shroud, as a fraction of the distance
    /// between neighbouring blade sections.
    pub blade_extension: f64,
}

impl Default for CadOptions {
    fn default() -> Self {
        Self {
            wall_thickness: None,
            bore_diameter: None,
            boss_diameter: None,
            fillet_radius: None,
            balance_holes: None,
            contour_points: 41,
            blade_extension: 0.5,
        }
    }
}

/// CAD solids of one impeller, in millimetres.
#[derive(Debug)]
pub struct ImpellerCad {
    /// One blade, extended beyond hub and shroud.
    pub blade: Shape,
    /// Revolved channel between hub and shroud.
    pub channel: Shape,
    /// Flow domain: channel minus blades.
    pub fluid: Shape,
    /// Blades trimmed to the channel.
    pub blades: Shape,
    /// Impeller material: hub disk, shroud and blades.
    pub impeller: Shape,
    /// Wall thickness used, m.
    pub wall_thickness: f64,
    /// Bore and boss diameters, m.
    pub bore_diameter: f64,
    pub boss_diameter: f64,
    /// Fillet radius at the blade roots, m; zero for sharp edges.
    pub fillet_radius: f64,
    pub balance_holes: Option<BalanceHoles>,
    /// What could not be built as asked.
    pub warnings: Vec<String>,
}

impl ImpellerCad {
    /// Volume of the flow domain, m³.
    pub fn fluid_volume(&self) -> f64 {
        self.fluid.volume() * 1e-9
    }

    /// Volume of the impeller material, m³.
    pub fn impeller_volume(&self) -> f64 {
        self.impeller.volume() * 1e-9
    }
}

/// Meridional point (z, r) in metres to a 3D point of the xz plane in millimetres.
fn xz(p: DVec2) -> DVec3 {
    DVec3::new(p.y * MM, 0.0, p.x * MM)
}

/// Closed planar wire through a polygon of edges given as point chains: each chain with
/// more than two points becomes a spline, two points a straight line.
fn profile_face(chains: &[Vec<DVec3>]) -> Result<Shape, CadError> {
    let edges = chains
        .iter()
        .map(|c| {
            if c.len() == 2 {
                occ::line_edge(c[0], c[1])
            } else {
                occ::spline_edge(c, false)
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    occ::planar_face(&occ::wire(&edges)?)
}

/// Section points extrapolated beyond `edge` away from `inner` by `factor`.
fn extrapolate(edge: &[DVec3], inner: &[DVec3], factor: f64) -> Vec<DVec3> {
    edge.iter()
        .zip(inner)
        .map(|(e, i)| *e + (*e - *i) * factor)
        .collect()
}

/// Closed wire of one blade section from its pressure and suction sides (leading to
/// trailing edge). Zero-thickness edges become a smooth nose of the spline; edges with
/// thickness are closed by straight lines.
fn section_wire(ps: &[DVec3], ss: &[DVec3]) -> Result<Shape, CadError> {
    let n = ps.len();
    let le_closed = ps[0].distance(ss[0]) < CLOSED_EDGE;
    let te_closed = ps[n - 1].distance(ss[n - 1]) < CLOSED_EDGE;
    let edges = match (le_closed, te_closed) {
        (true, true) => {
            // Periodic loop starting at the trailing edge: suction TE→LE, pressure LE→TE.
            let mut pts: Vec<DVec3> = ss.iter().rev().copied().collect();
            pts.extend_from_slice(&ps[1..n - 1]);
            vec![occ::spline_edge(&pts, true)?]
        }
        (true, false) => {
            let mut pts: Vec<DVec3> = ss.iter().rev().copied().collect();
            pts.extend_from_slice(&ps[1..]);
            vec![
                occ::spline_edge(&pts, false)?,
                occ::line_edge(ps[n - 1], ss[n - 1])?,
            ]
        }
        (false, true) => {
            let mut pts: Vec<DVec3> = ps.to_vec();
            pts.extend(ss[..n - 1].iter().rev());
            vec![
                occ::spline_edge(&pts, false)?,
                occ::line_edge(ss[0], ps[0])?,
            ]
        }
        (false, false) => vec![
            occ::spline_edge(ss, false)?,
            occ::line_edge(ss[n - 1], ps[n - 1])?,
            occ::spline_edge(&ps.iter().rev().copied().collect::<Vec<_>>(), false)?,
            occ::line_edge(ps[0], ss[0])?,
        ],
    };
    occ::wire(&edges)
}

fn scaled(points: Vec<DVec3>) -> Vec<DVec3> {
    points.into_iter().map(|p| p * MM).collect()
}

/// Lofted solid of one blade, extended beyond hub and shroud.
fn blade_solid(g: &ImpellerGeometry, extension: f64) -> Result<Shape, CadError> {
    let sign = g.blade.rotation_sign;
    let sides = |s: &BladeSection| (scaled(s.pressure_side(sign)), scaled(s.suction_side(sign)));
    let mut sections: Vec<(Vec<DVec3>, Vec<DVec3>)> = g.blade.sections.iter().map(sides).collect();
    let m = sections.len();
    if m < 2 {
        return Err(CadError::Input(
            "a blade needs at least two sections".into(),
        ));
    }
    let hub = (
        extrapolate(&sections[0].0, &sections[1].0, extension),
        extrapolate(&sections[0].1, &sections[1].1, extension),
    );
    let shroud = (
        extrapolate(&sections[m - 1].0, &sections[m - 2].0, extension),
        extrapolate(&sections[m - 1].1, &sections[m - 2].1, extension),
    );
    sections.insert(0, hub);
    sections.push(shroud);
    // A blunt trailing edge is a chord between two points at r₂, which would leave a
    // thin lens of fluid against the outlet surface. Extend blunt edges past r₂ along
    // the blade so that trimming with the channel cuts them exactly at d₂, as machining
    // the impeller to its outer diameter does.
    let reach = (g.basis.blade_thickness * MM).max(1.0);
    for (ps, ss) in &mut sections {
        let n = ps.len();
        if ps[n - 1].distance(ss[n - 1]) > CLOSED_EDGE {
            for side in [ps, ss] {
                let tip = side[n - 1];
                let direction = (tip - side[n - 2]).normalize();
                side.push(tip + direction * reach);
            }
        }
    }
    let wires = sections
        .iter()
        .map(|(ps, ss)| section_wire(ps, ss))
        .collect::<Result<Vec<_>, _>>()?;
    occ::loft(&wires)
}

/// Offsets a meridional curve (z, r) by `distance` along its normal on the side away
/// from `away_from`. Fails if the curvature radius is smaller than the offset.
fn offset_curve(points: &[DVec2], distance: f64, away_from: DVec2) -> Result<Vec<DVec2>, CadError> {
    let n = points.len();
    let tangent = |i: usize| (points[(i + 1).min(n - 1)] - points[i.saturating_sub(1)]).normalize();
    let mid = n / 2;
    let normal = |t: DVec2| DVec2::new(-t.y, t.x);
    let side = if normal(tangent(mid)).dot(away_from - points[mid]) > 0.0 {
        -1.0
    } else {
        1.0
    };
    let offset: Vec<DVec2> = (0..n)
        .map(|i| points[i] + normal(tangent(i)) * (side * distance))
        .collect();
    // An offset larger than the curvature radius reverses the curve locally.
    for i in 0..n - 1 {
        if (offset[i + 1] - offset[i]).dot(points[i + 1] - points[i]) <= 0.0 {
            return Err(CadError::Input(format!(
                "wall thickness {:.1} mm exceeds the curvature radius of the contour",
                distance * MM
            )));
        }
    }
    Ok(offset)
}

/// Builds blade, channel, flow domain and impeller solids from the geometry.
pub fn build_impeller(g: &ImpellerGeometry, options: &CadOptions) -> Result<ImpellerCad, CadError> {
    let b = &g.basis;
    let (rn, r2) = (b.hub_diameter / 2.0, b.outlet_diameter / 2.0);
    let length = g.contour.axial_length;
    let t = options
        .wall_thickness
        .unwrap_or_else(|| tp_design::impeller::default_wall_thickness(b.outlet_diameter));
    let rb = options.bore_diameter.map_or(rn, |d| d / 2.0);
    if !(t > 0.0 && t.is_finite()) {
        return Err(CadError::Input("wall thickness must be positive".into()));
    }
    if !(rb > 0.0 && rb <= rn) {
        return Err(CadError::Input(
            "the bore diameter must be positive and not larger than the hub diameter".into(),
        ));
    }
    let k = options.contour_points.max(5);
    let hub: Vec<DVec2> = g.contour.hub.sample(k);
    let shroud: Vec<DVec2> = g.contour.shroud.sample(k);
    let chain = |pts: &[DVec2]| pts.iter().map(|&p| xz(p)).collect::<Vec<_>>();
    let rev = |pts: &[DVec2]| pts.iter().rev().map(|&p| xz(p)).collect::<Vec<_>>();

    // Channel: hub (inlet → outlet), outlet, shroud (outlet → inlet), inlet.
    let channel = occ::revolve_z(&profile_face(&[
        chain(&hub),
        vec![xz(hub[k - 1]), xz(shroud[k - 1])],
        rev(&shroud),
        vec![xz(shroud[0]), xz(hub[0])],
    ])?)?;

    // Hub disk: a wall of thickness t behind the hub contour, from d₂ in to the boss,
    // and the boss from the bore out, up to the rear face (our design).
    let rear = length + t;
    let rboss = options
        .boss_diameter
        .map_or_else(|| rb + (0.25 * 2.0 * rb).max(t), |d| d / 2.0);
    if !(rboss > rb && rboss < r2 - t) {
        return Err(CadError::Input(format!(
            "the boss diameter must lie between the bore ({:.1} mm) and d₂ less the wall",
            2.0 * rb * MM
        )));
    }
    let pattern = options
        .balance_holes
        .as_ref()
        .map(|holes| hole_pattern(holes, rboss))
        .transpose()?;
    let behind = offset_curve(&hub, t, shroud[k / 2])?;
    // The back of the wall, from d₂ inward, until it meets the boss.
    let mut back: Vec<DVec2> = Vec::new();
    for i in (0..k).rev() {
        let p = behind[i];
        if p.y <= rboss {
            let q = back.last().copied().unwrap_or(p);
            let f = (q.y - rboss) / (q.y - p.y);
            back.push(q + (p - q) * f);
            break;
        }
        back.push(p);
    }
    if back.last().is_some_and(|p| p.y > rboss) {
        let end = *back.last().expect("not empty");
        back.push(DVec2::new(end.x, rboss));
    }
    let meet = *back.last().expect("not empty");
    // Shroud: the shroud contour thickened outwards (away from the hub).
    let outer = offset_curve(&shroud, t, hub[k / 2])?;

    // Everything the impeller fills when the flow domain is taken out: hub disk,
    // channel and shroud in one profile. Cutting the (rounded) flow domain out of it
    // meets no wall face, only the inlet and the outlet.
    let start = hub[0];
    let mut profile = Vec::new();
    if start.y - rb > 1e-9 {
        profile.push(vec![xz(DVec2::new(start.x, rb)), xz(start)]);
    }
    profile.extend([
        vec![xz(start), xz(shroud[0])],
        vec![xz(shroud[0]), xz(outer[0])],
        chain(&outer),
    ]);
    // Across the outlet at d₂: shroud wall, channel and hub wall.
    let rim = [outer[k - 1], shroud[k - 1], hub[k - 1], back[0]];
    profile.extend(
        rim.windows(2)
            .filter(|w| w[0].distance(w[1]) > 1e-9)
            .map(|w| vec![xz(w[0]), xz(w[1])]),
    );
    profile.extend([
        chain(&back),
        vec![xz(meet), xz(DVec2::new(rear, rboss))],
        vec![xz(DVec2::new(rear, rboss)), xz(DVec2::new(rear, rb))],
        vec![xz(DVec2::new(rear, rb)), xz(DVec2::new(start.x, rb))],
    ]);
    let material = occ::revolve_z(&profile_face(&profile)?)?;

    // Blades.
    let blade = blade_solid(g, options.blade_extension)?;
    let z = g.blade.count;
    let copies = (0..z)
        .map(|i| blade.rotated_z(2.0 * PI * f64::from(i) / f64::from(z)))
        .collect::<Result<Vec<_>, _>>()?;
    let all_blades = occ::compound(&copies)?;
    let mut fluid = channel.cut(&all_blades)?;
    let blades = channel.common(&all_blades)?;
    // The walls are the profile less the channel: their faces are those of the channel,
    // which the trimmed blades share.
    let mut impeller = material.cut(&channel)?.fuse(&blades)?;

    // Fillets where the blades meet hub and shroud: concave edges of the impeller and
    // convex ones of the flow domain, rounded on each with the same radius. (A boolean
    // between the two rounded solids would meet their common faces and fails.)
    let radius = options
        .fillet_radius
        .unwrap_or(0.5 * b.blade_thickness)
        .max(0.0);
    let mut fillet_radius = 0.0;
    let mut warnings = Vec::new();
    if radius > 0.0 {
        let walls = [g.contour.hub.sample(400), g.contour.shroud.sample(400)];
        let round = |shape: &Shape| -> Result<Shape, CadError> {
            let rounded = shape.filleted(&root_edges(shape, &walls), radius * MM)?;
            if rounded.is_valid() && rounded.solid_count() == 1 {
                Ok(rounded)
            } else {
                Err(CadError::Operation("no single valid solid".into()))
            }
        };
        match round(&impeller).and_then(|i| Ok((i, round(&fluid)?))) {
            Ok((rounded_impeller, rounded_fluid)) => {
                impeller = rounded_impeller;
                fluid = rounded_fluid;
                fillet_radius = radius;
            }
            Err(e) => warnings.push(format!(
                "the {:.1} mm fillets at the blade roots failed ({e}); the edges stay sharp",
                radius * MM
            )),
        }
    }

    // Balance holes, parallel to the axis through the hub wall, midway between the
    // boss and the rear seal and between two blades (our layout).
    let balance_holes = match (&options.balance_holes, pattern) {
        (Some(holes), Some(pattern)) => {
            let r = pattern.pitch_diameter / 2.0;
            let radius = holes.diameter / 2.0;
            let start = hub_z_at(&hub, r - radius) - holes.diameter;
            let height = rear + t - start;
            let first = blade_angle_at(g, r) + PI / f64::from(z);
            let cylinders = (0..holes.count)
                .map(|i| {
                    let angle = first + 2.0 * PI * f64::from(i) / f64::from(holes.count);
                    let base = DVec3::new(r * angle.cos(), r * angle.sin(), start) * MM;
                    occ::cylinder(base, DVec3::Z, radius * MM, height * MM)
                })
                .collect::<Result<Vec<_>, _>>()?;
            impeller = impeller.cut(&occ::compound(&cylinders)?)?;
            Some(pattern)
        }
        _ => None,
    };

    Ok(ImpellerCad {
        blade,
        channel,
        fluid,
        blades,
        impeller,
        wall_thickness: t,
        bore_diameter: 2.0 * rb,
        boss_diameter: 2.0 * rboss,
        fillet_radius,
        balance_holes,
        warnings,
    })
}

/// Indices of the edges of `shape` that run on one of the meridional `walls` (z, r in
/// m) and cross it both radially and around the axis: where the blades meet the wall.
/// The circles and seams of the walls themselves stay out.
fn root_edges(shape: &Shape, walls: &[Vec<DVec2>]) -> Vec<usize> {
    // Distance in mm from a meridional point to a wall polyline.
    let distance = |wall: &[DVec2], p: DVec2| -> f64 {
        wall.windows(2)
            .map(|w| {
                let (a, b) = (w[0] * MM, w[1] * MM);
                let t = ((p - a).dot(b - a) / (b - a).length_squared()).clamp(0.0, 1.0);
                p.distance(a + (b - a) * t)
            })
            .fold(f64::INFINITY, f64::min)
    };
    const TOLERANCE: f64 = 0.05;
    shape
        .edge_samples(9)
        .iter()
        .enumerate()
        .filter(|(_, points)| {
            if points.is_empty() {
                return false;
            }
            let meridional: Vec<DVec2> = points
                .iter()
                .map(|p| DVec2::new(p.z, p.x.hypot(p.y)))
                .collect();
            let on_wall = walls
                .iter()
                .any(|w| meridional.iter().all(|&m| distance(w, m) < TOLERANCE));
            let radii = meridional.iter().map(|m| m.y);
            let (lo, hi) = radii.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), r| {
                (lo.min(r), hi.max(r))
            });
            let around = points.windows(2).any(|w| {
                let turn = (w[0].y.atan2(w[0].x) - w[1].y.atan2(w[1].x)).abs();
                turn > 1e-4 && turn < PI
            });
            on_wall && hi - lo > TOLERANCE && around
        })
        .map(|(i, _)| i)
        .collect()
}

/// The pitch circle of the balance holes: midway between the boss and the rear seal,
/// where both leave room for a hole.
fn hole_pattern(holes: &BalanceHoleOptions, boss: f64) -> Result<BalanceHoles, CadError> {
    let radius = holes.diameter / 2.0;
    let seal = holes.seal_diameter / 2.0;
    let pitch = 0.5 * (boss + seal);
    if holes.count == 0
        || radius.is_nan()
        || radius <= 0.0
        || pitch - radius < boss
        || pitch + radius > seal
    {
        return Err(CadError::Input(format!(
            "{} balance holes of {:.1} mm do not fit between the boss ({:.1} mm) and the rear \
             seal ({:.1} mm)",
            holes.count,
            holes.diameter * MM,
            2.0 * boss * MM,
            holes.seal_diameter * MM
        )));
    }
    Ok(BalanceHoles {
        count: holes.count,
        diameter: holes.diameter,
        pitch_diameter: 2.0 * pitch,
    })
}

/// Axial position z of the hub contour (z, r) at radius `r`, m; the eye below it.
fn hub_z_at(hub: &[DVec2], r: f64) -> f64 {
    if r <= hub[0].y {
        return hub[0].x;
    }
    for w in hub.windows(2) {
        if (w[0].y..=w[1].y).contains(&r) {
            let f = (r - w[0].y) / (w[1].y - w[0].y);
            return w[0].x + f * (w[1].x - w[0].x);
        }
    }
    hub[hub.len() - 1].x
}

/// Angle about the axis of the blade's hub section at radius `r`; its leading edge
/// below that radius.
fn blade_angle_at(g: &ImpellerGeometry, r: f64) -> f64 {
    let mean = g.blade.sections[0].mean_line(g.blade.rotation_sign);
    let closest = mean
        .iter()
        .min_by(|a, b| {
            let da = (a.x.hypot(a.y) - r).abs();
            let db = (b.x.hypot(b.y) - r).abs();
            da.total_cmp(&db)
        })
        .expect("a blade section has points");
    closest.y.atan2(closest.x)
}

#[cfg(test)]
mod tests_support {
    use super::*;
    use tp_design::impeller::{self, ImpellerBasis, ImpellerGeometryInput};
    use tp_design::pump::{CentrifugalPumpInput, PumpDuty, design};

    pub(super) fn reference() -> ImpellerGeometry {
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
        let main = design(&duty, &input).unwrap();
        let basis = ImpellerBasis::from_pump(&main, &input, 1.0);
        impeller::build(&basis, &ImpellerGeometryInput::default()).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::reference;
    use super::*;
    use approx::assert_relative_eq;

    /// Volume of revolution of the closed meridional polygon (z, r) by Pappus.
    fn revolved_volume(polygon: &[DVec2]) -> f64 {
        let n = polygon.len();
        let (mut area, mut moment) = (0.0, 0.0);
        for i in 0..n {
            let (p, q) = (polygon[i], polygon[(i + 1) % n]);
            let cross = p.x * q.y - q.x * p.y;
            area += cross / 2.0;
            moment += cross * (p.y + q.y) / 6.0;
        }
        2.0 * PI * (moment / area) * area.abs()
    }

    /// One full build of the reference impeller with sharp edges and without holes,
    /// checked several ways (booleans on seven B-spline blades take a few seconds, so
    /// the build is shared), then one finished with fillets and balance holes.
    #[test]
    fn reference_impeller_solids() {
        let g = reference();
        let sharp = CadOptions {
            fillet_radius: Some(0.0),
            ..CadOptions::default()
        };
        let cad = build_impeller(&g, &sharp).unwrap();

        // Channel volume against Pappus on a fine polygon of the same contours.
        let mut polygon = g.contour.hub.sample(2000);
        polygon.extend(g.contour.shroud.sample(2000).into_iter().rev());
        let pappus = revolved_volume(&polygon);
        assert_relative_eq!(cad.channel.volume() * 1e-9, pappus, max_relative = 2e-3);

        // Booleans conserve volume: fluid + trimmed blades = channel.
        let (fluid, blades, channel) = (
            cad.fluid.volume(),
            cad.blades.volume(),
            cad.channel.volume(),
        );
        assert_relative_eq!(fluid + blades, channel, max_relative = 1e-4);

        // One connected flow domain, one solid per blade, one impeller part.
        assert_eq!(cad.fluid.solid_count(), 1);
        assert_eq!(cad.blades.solid_count(), g.blade.count as usize);
        assert_eq!(cad.impeller.solid_count(), 1);
        for shape in [&cad.blade, &cad.channel, &cad.fluid, &cad.impeller] {
            assert!(shape.is_valid());
        }

        // Each trimmed blade ≈ blade length × channel width × thickness.
        let one_blade = blades * 1e-9 / f64::from(g.blade.count);
        let s = &g.blade.sections[g.blade.sections.len() / 2];
        let le = s.leading_edge.position;
        let width_le = g
            .contour
            .shroud_at(g.contour.leading_edge.1)
            .distance(g.contour.hub_at(g.contour.leading_edge.0));
        let width = 0.5 * (width_le + g.basis.outlet_width);
        let estimate = s.mean_line_length() * width * g.basis.blade_thickness;
        assert!(
            (0.7..1.3).contains(&(one_blade / estimate)),
            "blade {one_blade:e} m³ vs estimate {estimate:e} m³ (LE at r = {})",
            le.y
        );

        // Impeller material: hub disk + shroud + blades, which only touch. The hub
        // disk is a wall with a boss around the bore.
        assert!(cad.impeller_volume() > blades * 1e-9);
        assert_relative_eq!(cad.bore_diameter, g.basis.hub_diameter);
        assert!(cad.boss_diameter > cad.bore_diameter && cad.fillet_radius == 0.0);

        // Fillets move material from the flow domain to the impeller: about
        // (1 − π/4)·r² on each side of every blade root, at hub and shroud.
        let holes = BalanceHoleOptions {
            count: 7,
            diameter: 0.0085,
            seal_diameter: 0.0841,
        };
        let finished = build_impeller(
            &g,
            &CadOptions {
                balance_holes: Some(holes),
                ..CadOptions::default()
            },
        )
        .unwrap();
        assert!(finished.warnings.is_empty(), "{:?}", finished.warnings);
        assert_relative_eq!(finished.fillet_radius, 0.5 * g.basis.blade_thickness);
        for shape in [&finished.fluid, &finished.impeller] {
            assert!(shape.is_valid() && shape.solid_count() == 1);
        }
        let r = finished.fillet_radius;
        let roots: f64 = [
            &g.blade.sections[0],
            &g.blade.sections[g.blade.sections.len() - 1],
        ]
        .iter()
        .map(|s| s.mean_line_length())
        .sum();
        let fillets = (1.0 - PI / 4.0) * r * r * 2.0 * roots * f64::from(g.blade.count);
        let lost = cad.fluid_volume() - finished.fluid_volume();
        assert!(
            (0.7..2.0).contains(&(lost / fillets)),
            "fillets {lost:e} m³ vs {fillets:e} m³"
        );
        // Each hole crosses the hub wall, a few wall thicknesses long at most.
        let gained = finished.impeller_volume() - cad.impeller_volume();
        let drilled = lost - gained;
        let hole = PI / 4.0 * holes.diameter * holes.diameter * cad.wall_thickness;
        assert!(
            (7.0 * hole..7.0 * 3.0 * hole).contains(&drilled),
            "drilled {drilled:e} m³ vs one hole through the wall {hole:e} m³"
        );
        let pattern = finished.balance_holes.unwrap();
        assert!(pattern.pitch_diameter > finished.boss_diameter && pattern.pitch_diameter < 0.0841);
    }

    #[test]
    fn rejects_bad_options() {
        let g = reference();
        let thick = CadOptions {
            wall_thickness: Some(0.2),
            ..Default::default()
        };
        assert!(build_impeller(&g, &thick).is_err());
        let wide_bore = CadOptions {
            bore_diameter: Some(1.0),
            ..Default::default()
        };
        assert!(build_impeller(&g, &wide_bore).is_err());
        // Holes that do not fit between boss and rear seal.
        let big_holes = CadOptions {
            balance_holes: Some(BalanceHoleOptions {
                count: 7,
                diameter: 0.04,
                seal_diameter: 0.0841,
            }),
            ..Default::default()
        };
        assert!(build_impeller(&g, &big_holes).is_err());
    }
}
