//! One flow domain for CFD: the inlet duct when modelled, the impeller channel less its
//! blades, the side chambers, a ring that joins them to the volute at the impeller
//! outlet, and the volute fluid. The duct shares the impeller inlet and the seal exit
//! of the front chamber.
//!
//! The chambers open at r₂ wider than the volute inlet b₃. The ring runs from r₂ out to
//! r₂ + Δ: at r₂ as wide as the two casing walls, at r₂ + Δ as wide as the ring of the
//! volute, the sides at 45° or steeper (our layout), so that it overlaps the volute ring
//! and shares the faces of the chambers and of the impeller outlet at r₂.

use tp_design::impeller::ImpellerGeometry;
use tp_design::volute::VoluteGeometry;
use tp_math::DVec2;

use crate::CadError;
use crate::impeller::ImpellerCad;
use crate::occ::{self, Shape};
use crate::volute::VoluteCad;

/// Model units per metre.
const MM: f64 = 1000.0;

/// Fuzzy tolerance of the booleans that join solids sharing faces, mm.
const JOIN_TOLERANCE: f64 = 1e-3;

/// The joining ring in the meridional plane, (z, r) in m: corners from the front casing
/// at r₂ round to the rear casing at r₂.
pub fn joining_ring(g: &ImpellerGeometry, volute: &VoluteGeometry) -> Result<[DVec2; 4], CadError> {
    let c = &g.contour;
    let b = &g.basis;
    let r2 = b.outlet_diameter / 2.0;
    let parallel = b.outlet_inclination.abs() < 1e-9
        && (b.outlet_hub_diameter - b.outlet_diameter).abs() < 1e-9 * b.outlet_diameter;
    if !parallel {
        return Err(CadError::Input(
            "the joined fluid needs a radial outlet edge parallel to the axis (d₂i = d₂): \
             the chambers then open at one radius"
                .into(),
        ));
    }
    let (front, rear) = (
        c.chambers.front_casing.start().x,
        c.chambers.rear_casing.start().x,
    );
    let mid = volute.basis.midplane;
    // A hair narrower than b₃, as the ring of the volute itself.
    let half = 0.995 * volute.design.inlet_width / 2.0;
    let (lo, hi) = ((mid - half).max(front), (mid + half).min(rear));
    if lo >= hi {
        return Err(CadError::Input(
            "the volute inlet does not face the impeller outlet between the casing walls".into(),
        ));
    }
    let reach = (lo - front).max(rear - hi).max(0.02 * r2);
    Ok([
        DVec2::new(front, r2),
        DVec2::new(rear, r2),
        DVec2::new(hi, r2 + reach),
        DVec2::new(lo, r2 + reach),
    ])
}

/// The joined flow domain of `impeller` (with designed side chambers), `volute` and,
/// when there is one, the inlet `duct` ahead of the impeller, and what to warn about:
/// separate solids when the faces did not join.
pub fn join_fluid(
    impeller: &ImpellerCad,
    g: &ImpellerGeometry,
    volute: &VoluteCad,
    volute_geometry: &VoluteGeometry,
    duct: Option<&Shape>,
) -> Result<(Shape, Vec<String>), CadError> {
    let chambers = impeller.side_chambers.as_ref().ok_or_else(|| {
        CadError::Input("the joined fluid needs the side chambers to be designed".into())
    })?;
    let corners = joining_ring(g, volute_geometry)?;
    let xz = |p: DVec2| tp_math::DVec3::new(p.y * MM, 0.0, p.x * MM);
    let edges = (0..4)
        .map(|i| occ::line_edge(xz(corners[i]), xz(corners[(i + 1) % 4])))
        .collect::<Result<Vec<_>, _>>()?;
    let ring = occ::revolve_z(&occ::planar_face(&occ::wire(&edges)?)?)?;
    let mut joined = impeller
        .fluid
        .fuse_fuzzy(&ring, JOIN_TOLERANCE)?
        .fuse_fuzzy(chambers, JOIN_TOLERANCE)?
        .fuse_fuzzy(&volute.fluid, JOIN_TOLERANCE)?;
    if let Some(duct) = duct {
        joined = joined.fuse_fuzzy(duct, JOIN_TOLERANCE)?;
    }
    let mut warnings = Vec::new();
    let solids = joined.solid_count();
    if solids != 1 || !joined.is_valid() {
        warnings.push(format!(
            "the joined fluid came out as {solids} solids{}; check the faces at r₂ and, \
             with the inlet duct, at the eye and the seal exit",
            if joined.is_valid() { "" } else { ", not valid" }
        ));
    }
    Ok((joined, warnings))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::impeller::tests_support::reference_with;
    use crate::{CadOptions, build_impeller, build_volute};
    use tp_design::impeller::ImpellerGeometryInput;
    use tp_design::outlet::ImpellerOutlet;
    use tp_design::volute::VoluteInput;

    #[test]
    fn the_chambers_join_the_impeller_and_the_volute_in_one_fluid() {
        let _slot = crate::test_slot::take();
        let mut input = ImpellerGeometryInput::default();
        input.meridional.chambers.enabled = true;
        let (g, main, duty) = reference_with(&input);
        let midplane = g.contour.hub.end().x - main.outlet_width / 2.0;
        let mut outlet = ImpellerOutlet::from_pump(&main, &duty, midplane, 1.0);
        outlet.sides = Some(g.sides(g.contour.material.wall_thickness, false));
        let vg = VoluteGeometry::build(&outlet, &VoluteInput::default()).unwrap();
        // The ring spans the casing walls at r₂ and narrows to the volute ring.
        let ring = joining_ring(&g, &vg).unwrap();
        let r2 = g.basis.outlet_diameter / 2.0;
        assert_eq!(ring[0].y, r2);
        assert!(ring[0].x < g.contour.shroud.end().x && ring[1].x > g.contour.hub.end().x);
        assert!(ring[2].y > r2);

        let sharp = CadOptions {
            fillet_radius: Some(0.0),
            ..CadOptions::default()
        };
        let cad = build_impeller(&g, &sharp).unwrap();
        let volute = build_volute(&vg).unwrap();
        let (joined, warnings) = join_fluid(&cad, &g, &volute, &vg, None).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(joined.solid_count(), 1);
        assert!(joined.is_valid());
        // More than its parts but the overlap of the rings.
        let parts = cad.fluid.volume() + cad.side_chambers.as_ref().unwrap().volume();
        assert!(joined.volume() > parts);
        assert!(joined.volume() < parts + volute.fluid.volume() * 1.1);

        // With the inlet duct, still one solid, larger by the duct.
        use tp_design::impeller::duct::{self, DuctInput};
        let built = duct::build(&g.contour, &g.basis, &DuctInput::default());
        let shape = crate::build_inlet_duct(&built).unwrap();
        let (joined_duct, warnings) = join_fluid(&cad, &g, &volute, &vg, Some(&shape)).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(joined_duct.solid_count(), 1);
        assert!(joined_duct.is_valid());
        let duct = shape.volume();
        assert!((joined_duct.volume() - joined.volume() - duct).abs() < 0.01 * duct);

        // A slanted outlet edge does not open the chambers at one radius.
        let mut slanted = g.clone();
        slanted.basis.outlet_hub_diameter = 0.9 * slanted.basis.outlet_diameter;
        assert!(joining_ring(&slanted, &vg).is_err());
    }
}
