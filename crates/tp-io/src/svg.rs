//! SVG drawings of the impeller: meridional section and front view.

use std::f64::consts::PI;

use tp_design::impeller::ImpellerGeometry;
use tp_math::DVec2;

const WIDTH: f64 = 800.0;
const MARGIN: f64 = 50.0;
const FOOTER: f64 = 40.0;

/// Maps model coordinates (m) to SVG pixels with uniform scale.
struct Frame {
    min: DVec2,
    scale: f64,
    height: f64,
}

impl Frame {
    /// Fits the box [min, max] into the drawing width; y grows upwards in the model.
    fn fit(min: DVec2, max: DVec2) -> Self {
        let size = max - min;
        let scale = (WIDTH - 2.0 * MARGIN) / size.x.max(size.y);
        let height = size.y * scale + 2.0 * MARGIN + FOOTER;
        Self { min, scale, height }
    }

    fn map(&self, p: DVec2) -> DVec2 {
        let q = (p - self.min) * self.scale;
        DVec2::new(MARGIN + q.x, self.height - FOOTER - MARGIN - q.y)
    }

    fn polyline(&self, points: &[DVec2], style: &str) -> String {
        let coords: Vec<String> = points
            .iter()
            .map(|&p| {
                let q = self.map(p);
                format!("{:.2},{:.2}", q.x, q.y)
            })
            .collect();
        format!(
            "<polyline points=\"{}\" fill=\"none\" {style}/>\n",
            coords.join(" ")
        )
    }

    fn open(&self, title: &str) -> String {
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{WIDTH}\" height=\"{h:.0}\" \
             viewBox=\"0 0 {WIDTH} {h:.0}\" font-family=\"sans-serif\" font-size=\"13\">\n\
             <title>{title}</title>\n\
             <rect width=\"100%\" height=\"100%\" fill=\"white\"/>\n",
            h = self.height
        )
    }

    fn caption(&self, text: &str) -> String {
        format!(
            "<text x=\"{MARGIN}\" y=\"{:.0}\" fill=\"#333\">{text}</text>\n",
            self.height - 15.0
        )
    }
}

const CONTOUR: &str = "stroke=\"#1a1a1a\" stroke-width=\"2.5\"";
const SPAN: &str = "stroke=\"#3b7dd8\" stroke-width=\"1\"";
const QUASI: &str = "stroke=\"#bbbbbb\" stroke-width=\"0.7\"";
const EDGE: &str = "stroke=\"#d33a2c\" stroke-width=\"2\"";
const AXIS: &str = "stroke=\"#888\" stroke-width=\"1\" stroke-dasharray=\"12 4 2 4\"";

/// Meridional section (z horizontal, r vertical): hub, shroud, spans, quasi-orthogonals,
/// leading and trailing edge.
pub fn meridional(g: &ImpellerGeometry) -> String {
    let r2 = g.basis.outlet_diameter / 2.0;
    let length = g.contour.axial_length;
    let frame = Frame::fit(
        DVec2::new(-0.1 * length, 0.0),
        DVec2::new(1.1 * length, 1.05 * r2),
    );
    let mut out = frame.open("Turbopump impeller: meridional section");
    out += &frame.polyline(
        &[
            DVec2::new(-0.1 * length, 0.0),
            DVec2::new(1.1 * length, 0.0),
        ],
        AXIS,
    );
    for q in g.channel.quasi_orthogonals.iter().step_by(4) {
        out += &frame.polyline(&[q.hub, q.shroud], QUASI);
    }
    for span in &g.channel.spans {
        out += &frame.polyline(&span.points, SPAN);
    }
    out += &frame.polyline(&g.contour.hub.sample(100), CONTOUR);
    out += &frame.polyline(&g.contour.shroud.sample(100), CONTOUR);
    let sections = &g.blade.sections;
    let le: Vec<DVec2> = sections.iter().map(|s| s.leading_edge.position).collect();
    let te: Vec<DVec2> = sections
        .iter()
        .map(|s| *s.meridional.last().expect("sections have points"))
        .collect();
    out += &frame.polyline(&le, EDGE);
    out += &frame.polyline(&te, EDGE);

    let label = |p: DVec2, text: &str, dy: f64| {
        let q = frame.map(p);
        format!(
            "<text x=\"{:.1}\" y=\"{:.1}\" fill=\"#d33a2c\">{text}</text>\n",
            q.x + 6.0,
            q.y + dy
        )
    };
    out += &label(le[le.len() / 2], "LE", 0.0);
    out += &label(te[te.len() / 2], "TE", 0.0);
    let b = &g.basis;
    out += &frame.caption(&format!(
        "Meridional section · d1 {:.1} mm · d2 {:.1} mm · b2 {:.1} mm · L {:.1} mm · {} spans",
        b.inlet_diameter * 1e3,
        b.outlet_diameter * 1e3,
        b.outlet_width * 1e3,
        length * 1e3,
        sections.len()
    ));
    out += "</svg>\n";
    out
}

/// Front view seen from the inlet looking downstream: blade outlines at hub (light)
/// and shroud (dark), eye and outlet circles.
pub fn front_view(g: &ImpellerGeometry) -> String {
    let r2 = g.basis.outlet_diameter / 2.0;
    let extent = 1.08 * r2;
    let frame = Frame::fit(DVec2::splat(-extent), DVec2::splat(extent));
    let mut out = frame.open("Turbopump impeller: front view");
    // Looking along +z the x axis points left: screen = (−x, y).
    let view = |x: f64, y: f64| DVec2::new(-x, y);
    let circle = |r: f64, style: &str| {
        let points: Vec<DVec2> = (0..=180)
            .map(|i| {
                let a = 2.0 * PI * f64::from(i) / 180.0;
                view(r * a.cos(), r * a.sin())
            })
            .collect();
        frame.polyline(&points, style)
    };
    out += &circle(r2, CONTOUR);
    out += &circle(g.basis.inlet_diameter / 2.0, QUASI);
    out += &circle(g.basis.hub_diameter / 2.0, QUASI);

    let blade = &g.blade;
    let sign = blade.rotation_sign;
    let outlines: Vec<(Vec<DVec2>, &str)> = [
        (
            blade.sections.first(),
            "stroke=\"#9db9e6\" stroke-width=\"1.2\"",
        ),
        (
            blade.sections.last(),
            "stroke=\"#1d4f9c\" stroke-width=\"1.6\"",
        ),
    ]
    .into_iter()
    .filter_map(|(s, style)| s.map(|s| (s, style)))
    .map(|(s, style)| {
        let mut outline = s.pressure_side(sign);
        outline.extend(s.suction_side(sign).into_iter().rev());
        outline.push(outline[0]);
        (
            outline.iter().map(|p| DVec2::new(p.x, p.y)).collect(),
            style,
        )
    })
    .collect();
    for i in 0..blade.count {
        let angle = 2.0 * PI * f64::from(i) / f64::from(blade.count);
        let (sin, cos) = angle.sin_cos();
        for (outline, style) in &outlines {
            let rotated: Vec<DVec2> = outline
                .iter()
                .map(|p| view(cos * p.x - sin * p.y, sin * p.x + cos * p.y))
                .collect();
            out += &frame.polyline(&rotated, style);
        }
    }
    let rotation = if sign > 0.0 {
        "clockwise"
    } else {
        "counter-clockwise"
    };
    out += &frame.caption(&format!(
        "Front view from the inlet · {} blades · rotation {rotation} · wrap {:.0}° (hub) / {:.0}° (shroud)",
        blade.count,
        blade.sections.first().map_or(0.0, |s| s.wrap_angle().to_degrees()),
        blade.sections.last().map_or(0.0, |s| s.wrap_angle().to_degrees()),
    ));
    out += "</svg>\n";
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::tests::reference;

    #[test]
    fn drawings_are_complete_svg_documents() {
        let g = reference();
        for svg in [meridional(&g), front_view(&g)] {
            assert!(svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""));
            assert!(svg.trim_end().ends_with("</svg>"));
            assert!(!svg.contains("NaN"));
        }
        let front = front_view(&g);
        // Two outlines (hub and shroud) per blade plus three circles.
        assert_eq!(
            front.matches("<polyline").count(),
            2 * g.blade.count as usize + 3
        );
    }

    #[test]
    fn frame_keeps_points_inside_the_canvas() {
        let frame = Frame::fit(DVec2::new(-1.0, 0.0), DVec2::new(3.0, 2.0));
        for p in [DVec2::new(-1.0, 0.0), DVec2::new(3.0, 2.0)] {
            let q = frame.map(p);
            assert!(q.x >= 0.0 && q.x <= WIDTH && q.y >= 0.0 && q.y <= frame.height);
        }
        // y grows upwards in the model, downwards on screen.
        assert!(frame.map(DVec2::new(0.0, 2.0)).y < frame.map(DVec2::new(0.0, 0.0)).y);
    }
}
