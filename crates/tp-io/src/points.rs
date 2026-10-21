//! Plain-text point lists of the impeller geometry.
//!
//! Format (one value per column, separated by spaces, SI units):
//!
//! ```text
//! # comment lines start with '#'
//! [hub]                      z r
//! [shroud]                   z r
//! [section span=<fraction>]  m z r theta beta thickness x y z (mean line)
//!                            x y z (pressure side) x y z (suction side)
//! ```
//!
//! z is the axial coordinate in the inflow direction, theta the angular position
//! measured from the trailing edge in the direction of rotation; Cartesian points
//! include the rotation sign, so they describe the first blade as built.

use std::fmt::Write;

use tp_design::impeller::ImpellerGeometry;
use tp_design::volute::VoluteGeometry;

/// Points per hub and shroud contour.
const CONTOUR_POINTS: usize = 101;

fn number(v: f64) -> String {
    format!("{v:.12e}")
}

/// The whole impeller as a text point list.
pub fn geometry_txt(g: &ImpellerGeometry) -> String {
    let mut out = String::new();
    let blade = &g.blade;
    // Writing to a String cannot fail.
    let _ = writeln!(out, "# Turbopump impeller geometry");
    let _ = writeln!(
        out,
        "# units: m and rad; z axial (inflow direction), r radius, theta from the trailing \
         edge in the direction of rotation"
    );
    let _ = writeln!(
        out,
        "# blades {}  rotation_sign {:+}  sections {}",
        blade.count,
        blade.rotation_sign,
        blade.sections.len()
    );
    for (name, curve) in [("hub", &g.contour.hub), ("shroud", &g.contour.shroud)] {
        let _ = writeln!(out, "[{name}]");
        let _ = writeln!(out, "# z r");
        for p in curve.sample(CONTOUR_POINTS) {
            let _ = writeln!(out, "{} {}", number(p.x), number(p.y));
        }
    }
    let sign = blade.rotation_sign;
    for s in &blade.sections {
        let _ = writeln!(out, "[section span={:.4}]", s.span);
        let _ = writeln!(
            out,
            "# m z r theta beta thickness x y z x_ps y_ps z_ps x_ss y_ss z_ss"
        );
        let (mean, ps, ss) = (
            s.mean_line(sign),
            s.pressure_side(sign),
            s.suction_side(sign),
        );
        for k in 0..s.m.len() {
            let values = [
                s.m[k],
                s.meridional[k].x,
                s.meridional[k].y,
                s.theta[k],
                s.beta[k],
                s.thickness[k],
                mean[k].x,
                mean[k].y,
                mean[k].z,
                ps[k].x,
                ps[k].y,
                ps[k].z,
                ss[k].x,
                ss[k].y,
                ss[k].z,
            ];
            let line: Vec<String> = values.iter().map(|&v| number(v)).collect();
            let _ = writeln!(out, "{}", line.join(" "));
        }
    }
    out
}

/// The volute as a text point list: one block per cross section and per diffuser
/// section, with Cartesian points of the closed outline.
pub fn volute_txt(g: &VoluteGeometry) -> String {
    let mut out = String::new();
    let d = &g.design;
    let _ = writeln!(out, "# Turbopump volute geometry");
    let _ = writeln!(
        out,
        "# units: m, m² and rad; wrap angle from the cutwater in the direction of rotation"
    );
    let _ = writeln!(
        out,
        "# cutwater diameter {} inlet width {} throat area {}",
        number(d.cutwater_diameter),
        number(d.inlet_width),
        number(d.throat.area)
    );
    let sign = g.basis.rotation_sign;
    for s in &g.sections {
        let _ = writeln!(
            out,
            "[section angle={} area={} outer_radius={}]",
            number(s.angle),
            number(s.size.area),
            number(s.size.outer_radius)
        );
        for p in s.points(sign) {
            let _ = writeln!(out, "{} {} {}", number(p.x), number(p.y), number(p.z));
        }
    }
    for (label, sections) in [
        ("inner volute section", &g.inner_sections),
        ("outer channel section", &g.outer_channel),
    ] {
        for s in sections {
            let _ = writeln!(
                out,
                "[{label} angle={} area={} outer_radius={}]",
                number(s.angle),
                number(s.size.area),
                number(s.size.outer_radius)
            );
            for p in s.points(sign) {
                let _ = writeln!(out, "{} {} {}", number(p.x), number(p.y), number(p.z));
            }
        }
    }
    // The rib of a double volute in the midplane, nose first.
    if let (Some(rib), Some(r)) = (&d.rib, &g.rib) {
        let _ = writeln!(
            out,
            "[rib outline thickness={} nose_ratio={}]",
            number(rib.thickness),
            number(rib.nose_ratio)
        );
        for p in r.outline() {
            let _ = writeln!(out, "{} {} {}", number(p.x), number(p.y), number(p.z));
        }
    }
    // A double volute's diffuser starts from both channels: its first outline is new.
    let first = if g.design.rib.is_some() { 0 } else { 1 };
    for (i, n) in g.nozzle.iter().enumerate().skip(first) {
        let _ = writeln!(out, "[diffuser section={i} area={}]", number(n.area));
        for p in &n.points {
            let _ = writeln!(out, "{} {} {}", number(p.x), number(p.y), number(p.z));
        }
    }
    out
}
