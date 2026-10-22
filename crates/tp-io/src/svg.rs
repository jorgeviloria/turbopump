//! SVG of a [`Drawing`]: the view fitted to a fixed width, its caption underneath.

use tp_math::DVec2;

use crate::drawing::{Drawing, Entity, Layer, LinePattern, Shape};

const WIDTH: f64 = 800.0;
const MARGIN: f64 = 50.0;
const FOOTER: f64 = 40.0;
/// SVG pixels per millimetre of line weight.
const PX_PER_MM_WEIGHT: f64 = 5.0;
/// Segments of a Bézier curve of degree above 3, which SVG paths cannot hold.
const BEZIER_SEGMENTS: usize = 100;

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

    fn points(&self, points: &[DVec2]) -> String {
        points
            .iter()
            .map(|&p| {
                let q = self.map(p);
                format!("{:.2},{:.2}", q.x, q.y)
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn color(rgb: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])
}

fn stroke(layer: &Layer) -> String {
    let width = f64::from(layer.weight) / 100.0 * PX_PER_MM_WEIGHT;
    let dash = match layer.pattern {
        LinePattern::Continuous => "",
        LinePattern::Center => " stroke-dasharray=\"12 4 2 4\"",
    };
    format!(
        "fill=\"none\" stroke=\"{}\" stroke-width=\"{width:.2}\"{dash}",
        color(layer.rgb)
    )
}

/// The SVG document of `drawing`.
pub fn write(drawing: &Drawing) -> String {
    let frame = Frame::fit(drawing.window.0, drawing.window.1);
    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{WIDTH}\" height=\"{h:.0}\" \
         viewBox=\"0 0 {WIDTH} {h:.0}\" font-family=\"sans-serif\" font-size=\"13\">\n\
         <title>{title}</title>\n\
         <rect width=\"100%\" height=\"100%\" fill=\"white\"/>\n",
        h = frame.height,
        title = drawing.title,
    );
    for entity in &drawing.entities {
        out += &element(&frame, entity);
    }
    out += &format!(
        "<text x=\"{MARGIN}\" y=\"{:.0}\" fill=\"#333\">{}</text>\n",
        frame.height - 15.0,
        drawing.caption
    );
    out += "</svg>\n";
    out
}

/// The SVG element of one entity.
fn element(frame: &Frame, entity: &Entity) -> String {
    let style = stroke(&entity.layer);
    match &entity.shape {
        Shape::Line(a, b) => format!(
            "<polyline points=\"{}\" {style}/>\n",
            frame.points(&[*a, *b])
        ),
        Shape::Polyline { points, closed } => {
            let mut points = points.clone();
            if *closed && let Some(&first) = points.first() {
                points.push(first);
            }
            format!("<polyline points=\"{}\" {style}/>\n", frame.points(&points))
        }
        Shape::Bezier(curve) => {
            // The mapping is affine, so the mapped control points give the same curve.
            let p: Vec<DVec2> = curve
                .control_points()
                .iter()
                .map(|&p| frame.map(p))
                .collect();
            let path = match p.as_slice() {
                [a, b] => format!("M{:.2},{:.2} L{:.2},{:.2}", a.x, a.y, b.x, b.y),
                [a, b, c] => format!(
                    "M{:.2},{:.2} Q{:.2},{:.2} {:.2},{:.2}",
                    a.x, a.y, b.x, b.y, c.x, c.y
                ),
                [a, b, c, e] => format!(
                    "M{:.2},{:.2} C{:.2},{:.2} {:.2},{:.2} {:.2},{:.2}",
                    a.x, a.y, b.x, b.y, c.x, c.y, e.x, e.y
                ),
                _ => {
                    let q = frame.points(&curve.sample(BEZIER_SEGMENTS + 1));
                    format!("M{}", q.replace(' ', " L"))
                }
            };
            format!("<path d=\"{path}\" {style}/>\n")
        }
        Shape::Circle { center, radius } => {
            let c = frame.map(*center);
            format!(
                "<circle cx=\"{:.2}\" cy=\"{:.2}\" r=\"{:.2}\" {style}/>\n",
                c.x,
                c.y,
                radius * frame.scale
            )
        }
        Shape::Text { at, height, value } => {
            let q = frame.map(*at);
            format!(
                "<text x=\"{:.1}\" y=\"{:.1}\" font-size=\"{:.1}\" fill=\"{}\">{value}</text>\n",
                q.x,
                q.y,
                height * frame.scale,
                color(entity.layer.rgb)
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drawing::{front_view, meridional};
    use crate::mesh::tests::reference;

    #[test]
    fn drawings_are_complete_svg_documents() {
        let g = reference();
        for svg in [write(&meridional(&g)), write(&front_view(&g))] {
            assert!(svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""));
            assert!(svg.trim_end().ends_with("</svg>"));
            assert!(!svg.contains("NaN"));
        }
        let front = write(&front_view(&g));
        // Two outlines (hub and shroud) per blade, and the outlet, eye and hub circles.
        assert_eq!(
            front.matches("<polyline").count(),
            2 * g.blade.count as usize
        );
        assert_eq!(front.matches("<circle").count(), 3);
    }

    #[test]
    fn contours_are_cubic_paths() {
        let g = reference();
        assert_eq!(g.contour.hub.segments()[0].degree(), 3);
        let svg = write(&meridional(&g));
        assert_eq!(svg.matches("<path d=\"M").count(), 2);
        assert_eq!(svg.matches(" C").count(), 2);
        assert!(svg.contains("stroke-dasharray=\"12 4 2 4\""));
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
