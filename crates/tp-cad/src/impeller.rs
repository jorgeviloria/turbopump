//! Solids of a radial impeller and its flow domain.
//!
//! - **Channel**: the meridional region between hub and shroud revolved through 360°.
//! - **Blade**: B-spline loft through the closed blade sections (pressure side, nose,
//!   suction side, trailing edge), extended slightly beyond hub and shroud so that
//!   boolean operations never meet coincident faces.
//! - **Flow domain**: channel minus all blades.
//! - **Impeller**: hub disk ∪ shroud ∪ (blades ∩ channel).
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

/// Options of the CAD model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CadOptions {
    /// Wall thickness of hub disk and shroud, m. `None`: max(3 mm, 0.02·d₂).
    pub wall_thickness: Option<f64>,
    /// Bore diameter of the hub, m. `None`: the hub diameter at the eye.
    pub bore_diameter: Option<f64>,
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
        .unwrap_or_else(|| (0.02 * b.outlet_diameter).max(0.003));
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

    // Hub disk: filled from the hub contour down to the bore, up to the rear face.
    let rear = length + t;
    let mut hub_chains = vec![
        chain(&hub),
        vec![xz(DVec2::new(length, r2)), xz(DVec2::new(rear, r2))],
        vec![xz(DVec2::new(rear, r2)), xz(DVec2::new(rear, rb))],
        vec![xz(DVec2::new(rear, rb)), xz(DVec2::new(0.0, rb))],
    ];
    if rn - rb > 1e-9 {
        hub_chains.push(vec![xz(DVec2::new(0.0, rb)), xz(DVec2::new(0.0, rn))]);
    }
    let hub_disk = occ::revolve_z(&profile_face(&hub_chains)?)?;

    // Shroud: the shroud contour thickened outwards (away from the hub).
    let mid_hub = hub[k / 2];
    let outer = offset_curve(&shroud, t, mid_hub)?;
    let shroud_solid = occ::revolve_z(&profile_face(&[
        chain(&shroud),
        vec![xz(shroud[k - 1]), xz(outer[k - 1])],
        rev(&outer),
        vec![xz(outer[0]), xz(shroud[0])],
    ])?)?;

    // Blades.
    let blade = blade_solid(g, options.blade_extension)?;
    let z = g.blade.count;
    let copies = (0..z)
        .map(|i| blade.rotated_z(2.0 * PI * f64::from(i) / f64::from(z)))
        .collect::<Result<Vec<_>, _>>()?;
    let all_blades = occ::compound(&copies)?;
    let fluid = channel.cut(&all_blades)?;
    let blades = channel.common(&all_blades)?;
    let impeller = hub_disk.fuse(&shroud_solid)?.fuse(&blades)?;

    Ok(ImpellerCad {
        blade,
        channel,
        fluid,
        blades,
        impeller,
        wall_thickness: t,
    })
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

    /// One full build of the reference impeller, checked several ways (booleans on
    /// seven B-spline blades take a few seconds, so the build is shared).
    #[test]
    fn reference_impeller_solids() {
        let g = reference();
        let cad = build_impeller(&g, &CadOptions::default()).unwrap();

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

        // Impeller material: hub disk + shroud + blades, which only touch.
        assert!(cad.impeller_volume() > blades * 1e-9);
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
    }
}
