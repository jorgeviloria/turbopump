//! CAD solids of a radial diffuser: its flow domain from the impeller outlet to d₄ and,
//! for a vaned diffuser, the vanes.

use tp_design::diffuser::DiffuserDesign;
use tp_math::{DVec2, DVec3};

use crate::CadError;
use crate::occ::{self, Shape};

/// Millimetres per metre: the CAD model is built in millimetres.
const MM: f64 = 1000.0;
/// Two points closer than this (mm) close a vane edge.
const CLOSED_EDGE: f64 = 1e-4;

/// CAD solids of one diffuser, in millimetres.
#[derive(Debug)]
pub struct DiffuserCad {
    /// Channel between the side walls, without the vanes.
    pub fluid: Shape,
    /// The vanes, trimmed to the side walls; `None` for a vaneless diffuser.
    pub vanes: Option<Shape>,
}

impl DiffuserCad {
    /// Volume of the flow domain, m³.
    pub fn fluid_volume(&self) -> f64 {
        self.fluid.volume() / MM.powi(3)
    }
}

/// Closed wire of one vane at height `z` (mm): outer side, blunt trailing edge, inner
/// side back to the leading edge.
fn vane_wire(outer: &[DVec2], inner: &[DVec2], z: f64) -> Result<Shape, CadError> {
    let at = |p: &DVec2| DVec3::new(p.x * MM, p.y * MM, z);
    let outer: Vec<DVec3> = outer.iter().map(at).collect();
    let inner: Vec<DVec3> = inner.iter().rev().map(at).collect();
    let n = outer.len();
    let mut edges = vec![occ::spline_edge(&outer, false)?];
    if outer[n - 1].distance(inner[0]) > CLOSED_EDGE {
        edges.push(occ::line_edge(outer[n - 1], inner[0])?);
    }
    edges.push(occ::spline_edge(&inner, false)?);
    if inner[n - 1].distance(outer[0]) > CLOSED_EDGE {
        edges.push(occ::line_edge(inner[n - 1], outer[0])?);
    }
    occ::wire(&edges)
}

/// Builds the flow domain and the vanes of a diffuser.
pub fn build_diffuser(d: &DiffuserDesign) -> Result<DiffuserCad, CadError> {
    let r2 = d.impeller_diameter / 2.0;
    let r3 = d.inlet_diameter / 2.0;
    let r4 = d.outlet_diameter / 2.0;
    let zm = d.midplane;
    // Meridional section of the channel in the xz plane, closed polygon.
    let mut radii = vec![r2];
    if r3 - r2 > 1e-6 {
        radii.push(r3);
    }
    radii.push(r4);
    let low: Vec<DVec3> = radii
        .iter()
        .map(|&r| DVec3::new(r * MM, 0.0, (zm - d.half_width(r)) * MM))
        .collect();
    let high: Vec<DVec3> = radii
        .iter()
        .rev()
        .map(|&r| DVec3::new(r * MM, 0.0, (zm + d.half_width(r)) * MM))
        .collect();
    let corners: Vec<DVec3> = low.into_iter().chain(high).collect();
    let edges = (0..corners.len())
        .map(|i| occ::line_edge(corners[i], corners[(i + 1) % corners.len()]))
        .collect::<Result<Vec<_>, _>>()?;
    let channel = occ::revolve_z(&occ::planar_face(&occ::wire(&edges)?)?)?;

    let Some(v) = &d.vanes else {
        return Ok(DiffuserCad {
            fluid: channel,
            vanes: None,
        });
    };
    // Vanes as prisms taller than the channel, then trimmed by it.
    let reach = d.half_width(r3).max(d.half_width(r4)) * 1.2 + 1e-3;
    let (z0, z1) = ((zm - reach) * MM, (zm + reach) * MM);
    let prisms = (0..v.count)
        .map(|k| {
            let (outer, inner) = v.geometry.sides(k, d.rotation_sign);
            occ::loft(&[
                vane_wire(&outer, &inner, z0)?,
                vane_wire(&outer, &inner, z1)?,
            ])
        })
        .collect::<Result<Vec<_>, _>>()?;
    let all = occ::compound(&prisms)?;
    Ok(DiffuserCad {
        fluid: channel.cut(&all)?,
        vanes: Some(all.common(&channel)?),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;
    use tp_design::diffuser::DiffuserInput;
    use tp_design::outlet::ImpellerOutlet;

    fn outlet() -> ImpellerOutlet {
        ImpellerOutlet {
            flow: 100.0 / 3600.0,
            impeller_diameter: 0.172,
            impeller_width: 0.0174,
            cu: 13.27,
            cm: 3.03,
            nq: 35.9,
            stage_head: 32.0,
            density: 998.2,
            midplane: 0.0378,
            rotation_sign: 1.0,
            impeller_blades: 7,
            sides: None,
        }
    }

    #[test]
    fn diffuser_solids_are_valid() {
        for vaned in [false, true] {
            let d = DiffuserDesign::new(&outlet(), &DiffuserInput::default(), vaned).unwrap();
            let cad = build_diffuser(&d).unwrap();
            assert!(cad.fluid.is_valid(), "vaned: {vaned}");
            assert_eq!(cad.fluid.solid_count(), 1, "vaned: {vaned}");
            // Annulus of constant width from d₂ to d₄, less the vanes.
            let (r2, r4) = (0.086, d.outlet_diameter / 2.0);
            let annulus = PI * (r4 * r4 - r2 * r2) * d.inlet_width;
            let fluid = cad.fluid.volume_with_tolerance(1e-3) / MM.powi(3);
            match &cad.vanes {
                None => assert!((fluid / annulus - 1.0).abs() < 1e-3, "{fluid} vs {annulus}"),
                Some(vanes) => {
                    assert!(vanes.is_valid());
                    assert_eq!(vanes.solid_count(), 12);
                    let metal = vanes.volume_with_tolerance(1e-3) / MM.powi(3);
                    assert!(
                        ((fluid + metal) / annulus - 1.0).abs() < 2e-3,
                        "{fluid} + {metal} vs {annulus}"
                    );
                }
            }
        }
    }
}
