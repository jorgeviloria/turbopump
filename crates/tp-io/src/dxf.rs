//! DXF of a [`Drawing`], through the `dxf` crate: AutoCAD 2000 format (AC1015), which
//! every CAD program of the last twenty years reads, in millimetres.
//!
//! Each drawing layer becomes a DXF layer with its colour and line type; entities take
//! their layer's colour and line type, and carry its line weight themselves (the crate
//! cannot set a layer's). Bézier curves become exact SPLINE entities: a Bézier curve of
//! degree p is the B-spline with p + 1 control points and the knots 0 and 1, each p + 1
//! times (Piegl and Tiller, *The NURBS Book*, 2nd ed., §2.2).

use ::dxf::entities::{Circle, Entity as DxfEntity, EntityType, Line, LwPolyline, Spline, Text};
use ::dxf::enums::{AcadVersion, DrawingUnits, Units};
use ::dxf::tables::{Layer as DxfLayer, LineType};
use ::dxf::{Color, LwPolylineVertex, Point};
use tp_math::DVec2;

use crate::drawing::{Drawing, Entity, LinePattern, Shape, TEXT, layers_of};

/// Millimetres per metre: the drawing model is in metres, the DXF in millimetres.
const MM: f64 = 1000.0;

/// Name and pattern (mm: dash > 0, gap < 0, dot 0) of the line type of `pattern`.
fn line_type(pattern: LinePattern) -> (&'static str, &'static [f64]) {
    match pattern {
        LinePattern::Continuous => ("CONTINUOUS", &[]),
        LinePattern::Center => ("CENTER", &[12.0, -3.0, 2.0, -3.0]),
    }
}

fn point(p: DVec2) -> Point {
    Point::new(p.x * MM, p.y * MM, 0.0)
}

/// The DXF entity of one drawing entity.
fn entity(e: &Entity) -> DxfEntity {
    let specific = match &e.shape {
        Shape::Line(a, b) => EntityType::Line(Line::new(point(*a), point(*b))),
        Shape::Polyline { points, closed } => {
            let mut polyline = LwPolyline {
                vertices: points
                    .iter()
                    .map(|p| LwPolylineVertex {
                        x: p.x * MM,
                        y: p.y * MM,
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            };
            polyline.set_is_closed(*closed);
            EntityType::LwPolyline(polyline)
        }
        Shape::Bezier(curve) => {
            let degree = curve.degree();
            let mut knots = vec![0.0; degree + 1];
            knots.extend(std::iter::repeat_n(1.0, degree + 1));
            let mut spline = Spline {
                degree_of_curve: i32::try_from(degree).expect("design curves have low degree"),
                knot_values: knots,
                control_points: curve.control_points().iter().map(|&p| point(p)).collect(),
                ..Default::default()
            };
            spline.set_is_planar(true);
            EntityType::Spline(spline)
        }
        Shape::Circle { center, radius } => {
            EntityType::Circle(Circle::new(point(*center), radius * MM))
        }
        Shape::Text { at, height, value } => EntityType::Text(Text {
            location: point(*at),
            text_height: height * MM,
            value: value.clone(),
            ..Default::default()
        }),
    };
    let mut out = DxfEntity::new(specific);
    out.common.layer = e.layer.name.to_owned();
    out.common.lineweight_enum_value = e.layer.weight;
    out
}

/// The DXF document of `drawing`, as text.
pub fn write(drawing: &Drawing) -> String {
    let mut out = ::dxf::Drawing::new();
    out.header.version = AcadVersion::R2000;
    out.header.default_drawing_units = Units::Millimeters;
    out.header.drawing_units = DrawingUnits::Metric;

    // The caption goes under the view, on the text layer.
    let mut entities = drawing.entities.clone();
    if !drawing.caption.is_empty() {
        entities.push(Entity {
            layer: TEXT,
            shape: Shape::Text {
                at: drawing.window.0 - DVec2::new(0.0, 2.0 * drawing.text_height),
                height: drawing.text_height,
                value: drawing.caption.clone(),
            },
        });
    }
    let layers = layers_of(&entities);

    // Extents, limits and the view the file opens with: the window and the caption.
    let low = point(drawing.window.0 - DVec2::new(0.0, 3.0 * drawing.text_height));
    let high = point(drawing.window.1);
    out.header.minimum_drawing_extents = low.clone();
    out.header.maximum_drawing_extents = high.clone();
    out.header.minimum_drawing_limits = low.clone();
    out.header.maximum_drawing_limits = high.clone();
    if let Some(view) = out.view_ports_mut().find(|v| v.name == "*ACTIVE") {
        let (width, height) = (high.x - low.x, high.y - low.y);
        view.view_center = Point::new(low.x + width / 2.0, low.y + height / 2.0, 0.0);
        view.view_height = 1.1 * height;
        view.view_port_aspect_ratio = width / height;
    }

    // CONTINUOUS is always defined; the others only when a layer uses them.
    if layers.iter().any(|l| l.pattern == LinePattern::Center) {
        let (name, lengths) = line_type(LinePattern::Center);
        out.add_line_type(LineType {
            name: name.to_owned(),
            description: "Center ____ _ ____ _".to_owned(),
            element_count: i32::try_from(lengths.len()).expect("short pattern"),
            total_pattern_length: lengths.iter().map(|l| l.abs()).sum(),
            dash_dot_space_lengths: lengths.to_vec(),
            ..Default::default()
        });
    }
    for layer in &layers {
        out.add_layer(DxfLayer {
            name: layer.name.to_owned(),
            color: Color::from_index(layer.aci),
            line_type_name: line_type(layer.pattern).0.to_owned(),
            ..Default::default()
        });
    }
    for e in &entities {
        out.add_entity(entity(e));
    }

    let mut bytes = Vec::new();
    out.save(&mut bytes)
        .expect("writing DXF into memory cannot fail");
    // AutoCAD 2000 DXF escapes every character beyond ASCII as \U+XXXX.
    String::from_utf8(bytes).expect("AutoCAD 2000 DXF is ASCII")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drawing::{CONTOUR, front_view, meridional};
    use crate::mesh::tests::reference;
    use approx::assert_relative_eq;

    fn read(text: &str) -> ::dxf::Drawing {
        ::dxf::Drawing::load(&mut text.as_bytes()).expect("our DXF reads back")
    }

    #[test]
    fn meridional_reads_back_with_exact_contours() {
        let g = reference();
        let text = write(&meridional(&g));
        assert!(text.contains("AC1015"));
        let back = read(&text);
        assert_eq!(back.header.default_drawing_units, Units::Millimeters);

        let splines: Vec<&Spline> = back
            .entities()
            .filter(|e| e.common.layer == CONTOUR.name)
            .filter_map(|e| match &e.specific {
                EntityType::Spline(s) => Some(s),
                _ => None,
            })
            .collect();
        assert_eq!(splines.len(), 2);
        for (spline, curve) in splines.iter().zip([&g.contour.hub, &g.contour.shroud]) {
            assert_eq!(spline.degree_of_curve, 3);
            assert_eq!(spline.knot_values, [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]);
            for (p, q) in spline.control_points.iter().zip(curve.control_points()) {
                assert_relative_eq!(p.x, q.x * MM, max_relative = 1e-12);
                assert_relative_eq!(p.y, q.y * MM, max_relative = 1e-12);
            }
        }

        let names: Vec<String> = back.layers().map(|l| l.name.clone()).collect();
        for name in [
            "AXIS",
            "CONSTRUCTION",
            "STREAMLINES",
            "CONTOUR",
            "BLADE-EDGES",
            "TEXT",
        ] {
            assert!(names.iter().any(|n| n == name), "{name} in {names:?}");
        }
        let axis = back
            .layers()
            .find(|l| l.name == "AXIS")
            .expect("axis layer");
        assert_eq!(axis.line_type_name, "CENTER");
        assert!(back.line_types().any(|t| t.name == "CENTER"));
    }

    #[test]
    fn caption_goes_under_the_view() {
        let g = reference();
        let d = meridional(&g);
        let back = read(&write(&d));
        let caption = back
            .entities()
            .filter(|e| e.common.layer == TEXT.name)
            .find_map(|e| match &e.specific {
                EntityType::Text(t) => Some(t),
                _ => None,
            })
            .expect("caption");
        assert!(caption.location.y < d.window.0.y * MM);
        // The extents hold the caption, so the file opens with everything in view.
        assert!(back.header.minimum_drawing_extents.y < caption.location.y);
        assert_relative_eq!(
            back.header.maximum_drawing_extents.x,
            d.window.1.x * MM,
            max_relative = 1e-9
        );
        let view = back
            .view_ports()
            .find(|v| v.name == "*ACTIVE")
            .expect("active view");
        assert!(view.view_height > d.window.1.y * MM);
        // AutoCAD 2000 text is ASCII: the middle dot is escaped, and reads back as itself.
        assert!(caption.value.starts_with("Meridional section · d1"));
    }

    #[test]
    fn front_view_outlines_are_closed_polylines() {
        let g = reference();
        let back = read(&write(&front_view(&g)));
        let closed = back
            .entities()
            .filter(|e| matches!(&e.specific, EntityType::LwPolyline(p) if p.is_closed()))
            .count();
        assert_eq!(closed, 2 * g.blade.count as usize);
        let circles = back
            .entities()
            .filter(|e| matches!(e.specific, EntityType::Circle(_)))
            .count();
        assert_eq!(circles, 3);
    }
}
