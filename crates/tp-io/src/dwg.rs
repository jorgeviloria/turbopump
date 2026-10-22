//! DWG of a [`Drawing`], written by Turbopump itself through the `acadrust` crate
//! (MPL-2.0, pure Rust): the layers, line types and entities of the DXF
//! ([`crate::dxf`]), in millimetres, in the DWG format of AutoCAD 2000 to 2018. No other
//! program is needed.
//!
//! Here layers carry their line weight and entities take it from their layer, as CAD
//! programs do. Bézier curves are exact SPLINE entities, as in the DXF.

use std::fmt;
use std::str::FromStr;

use acadrust::entities::{Circle, EntityType, Line, LwPolyline, Spline, Text};
use acadrust::tables::{Layer as DwgLayer, LineType, LineTypeElement};
use acadrust::types::{Color, DxfVersion, LineWeight, Vector2, Vector3};
use acadrust::{CadDocument, DwgWriter};
use tp_math::DVec2;

use crate::drawing::{Drawing, Entity, LinePattern, Shape, layers_of};
use crate::dxf::MM;

/// DWG file format, named by the AutoCAD release that introduced it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DwgVersion {
    R2000,
    R2004,
    R2007,
    R2010,
    R2013,
    #[default]
    R2018,
}

impl DwgVersion {
    pub const ALL: [DwgVersion; 6] = [
        DwgVersion::R2000,
        DwgVersion::R2004,
        DwgVersion::R2007,
        DwgVersion::R2010,
        DwgVersion::R2013,
        DwgVersion::R2018,
    ];

    pub fn year(self) -> u16 {
        match self {
            DwgVersion::R2000 => 2000,
            DwgVersion::R2004 => 2004,
            DwgVersion::R2007 => 2007,
            DwgVersion::R2010 => 2010,
            DwgVersion::R2013 => 2013,
            DwgVersion::R2018 => 2018,
        }
    }

    /// The file code, which is also the first six bytes of a DWG file.
    fn format(self) -> DxfVersion {
        match self {
            DwgVersion::R2000 => DxfVersion::AC1015,
            DwgVersion::R2004 => DxfVersion::AC1018,
            DwgVersion::R2007 => DxfVersion::AC1021,
            DwgVersion::R2010 => DxfVersion::AC1024,
            DwgVersion::R2013 => DxfVersion::AC1027,
            DwgVersion::R2018 => DxfVersion::AC1032,
        }
    }
}

impl fmt::Display for DwgVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.year())
    }
}

impl FromStr for DwgVersion {
    type Err = String;

    /// `2018`, `R2018` or `ACAD2018`, in any case.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let upper = s.trim().to_ascii_uppercase();
        let year = upper
            .strip_prefix("ACAD")
            .or_else(|| upper.strip_prefix('R'))
            .unwrap_or(&upper);
        DwgVersion::ALL
            .into_iter()
            .find(|v| v.year().to_string() == year)
            .ok_or_else(|| {
                let years: Vec<String> = DwgVersion::ALL.iter().map(|v| v.to_string()).collect();
                format!("DWG version must be one of {}, got {s:?}", years.join(", "))
            })
    }
}

fn point(p: DVec2) -> Vector3 {
    Vector3::new(p.x * MM, p.y * MM, 0.0)
}

fn point2(p: DVec2) -> Vector2 {
    Vector2::new(p.x * MM, p.y * MM)
}

/// The DWG entity of one drawing entity.
fn entity(e: &Entity) -> EntityType {
    let mut out = match &e.shape {
        Shape::Line(a, b) => EntityType::Line(Line::from_points(point(*a), point(*b))),
        Shape::Polyline { points, closed } => {
            let mut polyline = LwPolyline::from_points(points.iter().map(|&p| point2(p)).collect());
            if *closed {
                polyline.close();
            }
            EntityType::LwPolyline(polyline)
        }
        Shape::Bezier(curve) => {
            // With degree + 1 control points the clamped knots are the Bézier ones.
            let degree = i32::try_from(curve.degree()).expect("design curves have low degree");
            let mut spline = Spline::from_control_points(
                degree,
                curve.control_points().iter().map(|&p| point(p)).collect(),
            );
            spline.flags.planar = true;
            EntityType::Spline(spline)
        }
        Shape::Circle { center, radius } => {
            let c = point(*center);
            EntityType::Circle(Circle::from_coords(c.x, c.y, 0.0, radius * MM))
        }
        Shape::Text { at, height, value } => {
            EntityType::Text(Text::with_value(value.clone(), point(*at)).with_height(height * MM))
        }
    };
    out.common_mut().layer = e.layer.name.to_owned();
    out
}

/// The DWG file of `drawing` in `version`.
pub fn write(drawing: &Drawing, version: DwgVersion) -> Result<Vec<u8>, String> {
    let mut doc = CadDocument::with_version(version.format());
    doc.header.insertion_units = 4; // millimetres
    doc.header.measurement = 1; // metric

    let entities = drawing.with_caption();
    let layers = layers_of(&entities);

    // Limits and the view the file opens with: the window and the caption. The writer
    // works out the extents from the entities itself.
    let (low, high) = drawing.extents();
    doc.header.model_space_limits_min = point2(low);
    doc.header.model_space_limits_max = point2(high);
    if let Some(view) = doc.vports.get_mut("*Active") {
        let size = (high - low) * MM;
        view.view_center = point2((low + high) / 2.0);
        view.view_height = 1.1 * size.y;
        view.aspect_ratio = size.x / size.y;
    }

    // Continuous is always there; the others only when a layer uses them.
    if layers.iter().any(|l| l.pattern == LinePattern::Center) {
        let (name, description, lengths) = LinePattern::Center.line_type();
        let mut line_type = LineType::new(name);
        line_type.handle = doc.allocate_handle();
        line_type.description = description.to_owned();
        for &length in lengths {
            line_type.add_element(if length < 0.0 {
                LineTypeElement::space(length)
            } else {
                LineTypeElement::dash(length)
            });
        }
        line_type.pattern_length = lengths.iter().map(|l| l.abs()).sum();
        doc.line_types.add(line_type)?;
    }
    for layer in &layers {
        let mut out = DwgLayer::new(layer.name);
        out.handle = doc.allocate_handle();
        out.color = Color::Index(layer.aci);
        out.line_type = match layer.pattern {
            LinePattern::Continuous => LineType::continuous().name,
            LinePattern::Center => LinePattern::Center.line_type().0.to_owned(),
        };
        out.line_weight = LineWeight::Value(layer.weight);
        doc.layers.add(out)?;
    }
    for e in &entities {
        doc.add_entity(entity(e)).map_err(|e| e.to_string())?;
    }
    DwgWriter::write_to_vec(&doc).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drawing::{BLADE_EDGES, CONTOUR, TEXT, front_view, meridional};
    use crate::mesh::tests::reference;
    use acadrust::DwgReader;
    use approx::assert_relative_eq;

    fn read(bytes: Vec<u8>) -> CadDocument {
        DwgReader::from_stream(std::io::Cursor::new(bytes))
            .read()
            .expect("our DWG reads back")
    }

    #[test]
    fn versions_parse_in_every_spelling() {
        for text in ["2013", "R2013", "acad2013", " ACAD2013 "] {
            assert_eq!(text.parse::<DwgVersion>(), Ok(DwgVersion::R2013));
        }
        assert!("2019".parse::<DwgVersion>().is_err());
        assert_eq!(DwgVersion::default().to_string(), "2018");
    }

    #[test]
    fn every_version_reads_back_with_its_file_code() {
        let g = reference();
        let d = meridional(&g);
        for version in DwgVersion::ALL {
            let bytes = write(&d, version).expect("written");
            let code = version.format().as_str();
            assert_eq!(&bytes[..6], code.as_bytes(), "{version}");
            let back = read(bytes);
            assert_eq!(back.version, version.format(), "{version}");
            assert_eq!(back.entities().count(), d.with_caption().len(), "{version}");
        }
    }

    #[test]
    fn meridional_reads_back_with_exact_contours() {
        let g = reference();
        let d = meridional(&g);
        let back = read(write(&d, DwgVersion::R2018).expect("written"));
        assert_eq!(back.header.insertion_units, 4);

        let splines: Vec<&Spline> = back
            .entities()
            .filter_map(|e| match e {
                EntityType::Spline(s) if s.common.layer == CONTOUR.name => Some(s),
                _ => None,
            })
            .collect();
        assert_eq!(splines.len(), 2);
        for (spline, curve) in splines.iter().zip([&g.contour.hub, &g.contour.shroud]) {
            assert_eq!(spline.degree, 3);
            assert_eq!(spline.knots, [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]);
            for (p, q) in spline
                .control_points
                .iter()
                .zip(curve.segments()[0].control_points())
            {
                assert_relative_eq!(p.x, q.x * MM, max_relative = 1e-12);
                assert_relative_eq!(p.y, q.y * MM, max_relative = 1e-12);
            }
        }

        let contour = back.layers.get(CONTOUR.name).expect("contour layer");
        assert_eq!(contour.color, Color::Index(CONTOUR.aci));
        assert_eq!(contour.line_weight, LineWeight::Value(CONTOUR.weight));
        let axis = back.layers.get("AXIS").expect("axis layer");
        assert!(axis.line_type.eq_ignore_ascii_case("CENTER"));
        let center = back.line_types.get("CENTER").expect("center line type");
        let lengths: Vec<f64> = center.elements.iter().map(|e| e.length).collect();
        assert_eq!(lengths, [12.0, -3.0, 2.0, -3.0]);
        assert!(back.layers.get(BLADE_EDGES.name).is_some());
    }

    #[test]
    fn caption_and_extents_read_back() {
        let g = reference();
        let d = meridional(&g);
        let back = read(write(&d, DwgVersion::R2018).expect("written"));
        let caption = back
            .entities()
            .find_map(|e| match e {
                EntityType::Text(t) if t.common.layer == TEXT.name => Some(t),
                _ => None,
            })
            .expect("caption");
        assert!(caption.value.starts_with("Meridional section · d1"));
        assert!(caption.insertion_point.y < d.window.0.y * MM);
        // The extents hold everything drawn, the caption's text box included.
        let extents = (
            back.header.model_space_extents_min,
            back.header.model_space_extents_max,
        );
        assert!(extents.0.y <= caption.insertion_point.y);
        assert!(extents.1.x >= d.window.1.x * MM * (1.0 - 1e-9));
        let (low, high) = d.extents();
        assert_relative_eq!(back.header.model_space_limits_min.y, low.y * MM);
        assert_relative_eq!(back.header.model_space_limits_max.x, high.x * MM);
        let view = back.vports.get("*Active").expect("active view");
        assert!(view.view_height > (high.y - low.y) * MM);
    }

    #[test]
    fn front_view_outlines_are_closed_polylines() {
        let g = reference();
        let back = read(write(&front_view(&g), DwgVersion::R2000).expect("written"));
        let closed = back
            .entities()
            .filter(|e| matches!(e, EntityType::LwPolyline(p) if p.is_closed))
            .count();
        assert_eq!(closed, 2 * g.blade.count as usize);
        let circles = back
            .entities()
            .filter(|e| matches!(e, EntityType::Circle(_)))
            .count();
        assert_eq!(circles, 3);
    }
}
