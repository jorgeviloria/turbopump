//! Wall-clock cost of the volute CAD for a few designs, to compare changes to the
//! OCCT bridge. Run alone, in release:
//! `cargo test -p tp-cad --release --test timing -- --ignored --nocapture`

use std::time::Instant;

use tp_cad::{build_volute, build_volute_casing};
use tp_design::impeller::edge::EdgeShape;
use tp_design::outlet::ImpellerOutlet;
use tp_design::volute::{
    CrossSectionInput, ExternalRound, SectionShape, VoluteGeometry, VoluteInput, VoluteKind,
};

fn basis() -> ImpellerOutlet {
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
#[ignore = "timing benchmark, minutes long"]
fn volute_cad_timings() {
    type Edit = fn(&mut VoluteInput);
    let cases: [(&str, Edit); 4] = [
        ("trapezoid", |_| {}),
        ("round external", |v| {
            v.cross_section =
                CrossSectionInput::single(SectionShape::RoundExternal(ExternalRound::default()))
        }),
        ("double", |v| v.setup.kind = VoluteKind::Double),
        ("tapered nose", |v| v.cutwater.nose_shape = EdgeShape::TAPER),
    ];
    let only = std::env::var("TP_CASE").ok();
    for (name, edit) in cases {
        if only.as_deref().is_some_and(|c| c != name) {
            continue;
        }
        let mut input = VoluteInput::default();
        edit(&mut input);
        let g = VoluteGeometry::build(&basis(), &input).unwrap();
        let start = Instant::now();
        let cad = build_volute(&g);
        let volute = start.elapsed();
        let casing = cad.as_ref().ok().map(|cad| {
            let start = Instant::now();
            let ok = build_volute_casing(&g, cad).is_ok();
            (start.elapsed(), ok)
        });
        println!(
            "{name}: volute {volute:.1?} ({}), casing {:?}",
            if cad.is_ok() { "ok" } else { "failed" },
            casing
        );
    }
}
