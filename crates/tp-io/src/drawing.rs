//! Two-dimensional drawings of the impeller, independent of the file format: layers and
//! entities in model coordinates (metres), written by [`crate::svg`], [`crate::dxf`] and
//! [`crate::dwg`].
//!
//! Curves the design defines exactly stay exact: the hub and shroud contours are the
//! design's own Bézier curves, circles are circles. Curves the design computes point by
//! point (streamlines, blade sections) are polylines through its points.

use std::f64::consts::PI;

use tp_design::impeller::ImpellerGeometry;
use tp_math::{Bezier, DVec2};

/// Line pattern of a layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinePattern {
    Continuous,
    /// Long dash, gap, short dash, gap: axes of symmetry.
    Center,
}

impl LinePattern {
    /// Name of the line type in CAD files, its description and its elements in
    /// millimetres: dash > 0, gap < 0.
    pub fn line_type(self) -> (&'static str, &'static str, &'static [f64]) {
        match self {
            LinePattern::Continuous => ("CONTINUOUS", "Solid line", &[]),
            LinePattern::Center => ("CENTER", "Center ____ _ ____ _", &[12.0, -3.0, 2.0, -3.0]),
        }
    }
}

/// A layer: its name in CAD files and how its entities are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layer {
    pub name: &'static str,
    /// AutoCAD Color Index, for DXF.
    pub aci: u8,
    /// Colour on screen, sRGB.
    pub rgb: [u8; 3],
    /// Line weight in hundredths of a millimetre, one of the standard DXF values.
    pub weight: i16,
    pub pattern: LinePattern,
}

pub const CONTOUR: Layer = Layer {
    name: "CONTOUR",
    aci: 7,
    rgb: [0x1a, 0x1a, 0x1a],
    weight: 50,
    pattern: LinePattern::Continuous,
};
pub const AXIS: Layer = Layer {
    name: "AXIS",
    aci: 8,
    rgb: [0x88, 0x88, 0x88],
    weight: 18,
    pattern: LinePattern::Center,
};
/// Quasi-orthogonals and reference circles.
pub const CONSTRUCTION: Layer = Layer {
    name: "CONSTRUCTION",
    aci: 9,
    rgb: [0xbb, 0xbb, 0xbb],
    weight: 13,
    pattern: LinePattern::Continuous,
};
pub const STREAMLINES: Layer = Layer {
    name: "STREAMLINES",
    aci: 5,
    rgb: [0x3b, 0x7d, 0xd8],
    weight: 18,
    pattern: LinePattern::Continuous,
};
/// Leading and trailing edges and their labels.
pub const BLADE_EDGES: Layer = Layer {
    name: "BLADE-EDGES",
    aci: 1,
    rgb: [0xd3, 0x3a, 0x2c],
    weight: 35,
    pattern: LinePattern::Continuous,
};
pub const BLADE_HUB: Layer = Layer {
    name: "BLADE-HUB",
    aci: 4,
    rgb: [0x9d, 0xb9, 0xe6],
    weight: 25,
    pattern: LinePattern::Continuous,
};
pub const BLADE_SHROUD: Layer = Layer {
    name: "BLADE-SHROUD",
    aci: 5,
    rgb: [0x1d, 0x4f, 0x9c],
    weight: 35,
    pattern: LinePattern::Continuous,
};
/// The caption.
pub const TEXT: Layer = Layer {
    name: "TEXT",
    aci: 7,
    rgb: [0x33, 0x33, 0x33],
    weight: 18,
    pattern: LinePattern::Continuous,
};

/// The geometry of an entity, in model coordinates (m).
#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    Line(DVec2, DVec2),
    Polyline {
        points: Vec<DVec2>,
        closed: bool,
    },
    Bezier(Bezier<DVec2>),
    Circle {
        center: DVec2,
        radius: f64,
    },
    /// Text whose baseline starts at `at`; `height` in model units.
    Text {
        at: DVec2,
        height: f64,
        value: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entity {
    pub layer: Layer,
    pub shape: Shape,
}

/// One view on one sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct Drawing {
    pub title: String,
    /// One line under the view: what it shows and its main dimensions.
    pub caption: String,
    /// The model window the view shows, lower-left and upper-right corners.
    pub window: (DVec2, DVec2),
    /// Height of labels and caption, model units.
    pub text_height: f64,
    pub entities: Vec<Entity>,
}

impl Drawing {
    /// An empty drawing of `window`, with text scaled to it.
    fn new(title: &str, window: (DVec2, DVec2)) -> Self {
        let size = window.1 - window.0;
        Self {
            title: title.to_owned(),
            caption: String::new(),
            window,
            text_height: TEXT_FRACTION * size.x.max(size.y),
            entities: Vec::new(),
        }
    }

    fn add(&mut self, layer: Layer, shape: Shape) {
        self.entities.push(Entity { layer, shape });
    }

    fn polyline(&mut self, layer: Layer, points: Vec<DVec2>, closed: bool) {
        self.add(layer, Shape::Polyline { points, closed });
    }

    /// The layers in use, each once, in order of first use.
    pub fn layers(&self) -> Vec<Layer> {
        layers_of(&self.entities)
    }

    /// The entities and the caption under the view, on the text layer: what a CAD file
    /// holds, where the caption is not a page footer.
    pub fn with_caption(&self) -> Vec<Entity> {
        let mut entities = self.entities.clone();
        if !self.caption.is_empty() {
            entities.push(Entity {
                layer: TEXT,
                shape: Shape::Text {
                    at: self.window.0 - DVec2::new(0.0, 2.0 * self.text_height),
                    height: self.text_height,
                    value: self.caption.clone(),
                },
            });
        }
        entities
    }

    /// Lower-left and upper-right corners of the window with the caption under it.
    pub fn extents(&self) -> (DVec2, DVec2) {
        (
            self.window.0 - DVec2::new(0.0, 3.0 * self.text_height),
            self.window.1,
        )
    }
}

/// The layers of `entities`, each once, in order of first use.
pub(crate) fn layers_of(entities: &[Entity]) -> Vec<Layer> {
    let mut layers: Vec<Layer> = Vec::new();
    for layer in entities.iter().map(|e| e.layer) {
        if !layers.iter().any(|l| l.name == layer.name) {
            layers.push(layer);
        }
    }
    layers
}

/// Text height as a fraction of the larger side of the window.
const TEXT_FRACTION: f64 = 0.018;

/// Meridional section, z horizontal and r vertical: axis, quasi-orthogonals, spans,
/// hub and shroud, leading and trailing edge.
pub fn meridional(g: &ImpellerGeometry) -> Drawing {
    let r2 = g.basis.outlet_diameter / 2.0;
    let length = g.contour.axial_length;
    let mut d = Drawing::new(
        "Turbopump impeller: meridional section",
        (
            DVec2::new(-0.1 * length, 0.0),
            DVec2::new(1.1 * length, 1.05 * r2),
        ),
    );
    d.add(
        AXIS,
        Shape::Line(
            DVec2::new(-0.1 * length, 0.0),
            DVec2::new(1.1 * length, 0.0),
        ),
    );
    for q in g.channel.quasi_orthogonals.iter().step_by(4) {
        d.add(CONSTRUCTION, Shape::Line(q.hub, q.shroud));
    }
    for span in &g.channel.spans {
        d.polyline(STREAMLINES, span.points.clone(), false);
    }
    for wall in [&g.contour.hub, &g.contour.shroud] {
        for segment in wall.segments() {
            d.add(
                CONTOUR,
                if segment.degree() == 1 {
                    Shape::Line(segment.start(), segment.end())
                } else {
                    Shape::Bezier(segment.clone())
                },
            );
        }
    }

    let sections = &g.blade.sections;
    let le: Vec<DVec2> = sections.iter().map(|s| s.leading_edge.position).collect();
    let te: Vec<DVec2> = sections
        .iter()
        .map(|s| *s.meridional.last().expect("sections have points"))
        .collect();
    let gap = DVec2::new(0.5 * d.text_height, 0.0);
    for (edge, name) in [(le, "LE"), (te, "TE")] {
        let at = edge[edge.len() / 2] + gap;
        d.polyline(BLADE_EDGES, edge, false);
        d.add(
            BLADE_EDGES,
            Shape::Text {
                at,
                height: d.text_height,
                value: name.to_owned(),
            },
        );
    }

    let b = &g.basis;
    d.caption = format!(
        "Meridional section · d1 {:.1} mm · d2 {:.1} mm · b2 {:.1} mm · L {:.1} mm · {} spans",
        b.inlet_diameter * 1e3,
        b.outlet_diameter * 1e3,
        b.outlet_width * 1e3,
        length * 1e3,
        sections.len()
    );
    d
}

/// Front view seen from the inlet looking downstream: outlet, eye and hub circles, and
/// the blade outlines at hub (light) and shroud (dark).
pub fn front_view(g: &ImpellerGeometry) -> Drawing {
    let r2 = g.basis.outlet_diameter / 2.0;
    let extent = 1.08 * r2;
    let mut d = Drawing::new(
        "Turbopump impeller: front view",
        (DVec2::splat(-extent), DVec2::splat(extent)),
    );
    for (layer, radius) in [
        (CONTOUR, r2),
        (CONSTRUCTION, g.basis.inlet_diameter / 2.0),
        (CONSTRUCTION, g.basis.hub_diameter / 2.0),
    ] {
        d.add(
            layer,
            Shape::Circle {
                center: DVec2::ZERO,
                radius,
            },
        );
    }

    let blade = &g.blade;
    let sign = blade.rotation_sign;
    let outlines: Vec<(Layer, Vec<DVec2>)> = [
        (BLADE_HUB, blade.sections.first()),
        (BLADE_SHROUD, blade.sections.last()),
    ]
    .into_iter()
    .filter_map(|(layer, s)| s.map(|s| (layer, s)))
    .map(|(layer, s)| {
        let mut outline = s.pressure_side(sign);
        outline.extend(s.suction_side(sign).into_iter().rev());
        (
            layer,
            outline.iter().map(|p| DVec2::new(p.x, p.y)).collect(),
        )
    })
    .collect();
    for i in 0..blade.count {
        let angle = 2.0 * PI * f64::from(i) / f64::from(blade.count);
        let (sin, cos) = angle.sin_cos();
        for (layer, outline) in &outlines {
            // Looking along +z the x axis points left: view = (−x, y).
            let rotated = outline
                .iter()
                .map(|p| DVec2::new(-(cos * p.x - sin * p.y), sin * p.x + cos * p.y))
                .collect();
            d.polyline(*layer, rotated, true);
        }
    }

    let rotation = if sign > 0.0 {
        "clockwise"
    } else {
        "counter-clockwise"
    };
    d.caption = format!(
        "Front view from the inlet · {} blades · rotation {rotation} · wrap {:.0}° (hub) / {:.0}° (shroud)",
        blade.count,
        blade
            .sections
            .first()
            .map_or(0.0, |s| s.wrap_angle().to_degrees()),
        blade
            .sections
            .last()
            .map_or(0.0, |s| s.wrap_angle().to_degrees()),
    );
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::tests::reference;

    #[test]
    fn meridional_keeps_the_contours_exact() {
        let g = reference();
        let d = meridional(&g);
        let curves: Vec<&Bezier<DVec2>> = d
            .entities
            .iter()
            .filter_map(|e| match &e.shape {
                Shape::Bezier(b) => Some(b),
                _ => None,
            })
            .collect();
        assert_eq!(
            curves,
            [
                &g.contour.hub.segments()[0],
                &g.contour.shroud.segments()[0]
            ]
        );
        assert_eq!(
            d.entities.iter().filter(|e| e.layer == STREAMLINES).count(),
            g.channel.spans.len()
        );
        assert!(d.caption.starts_with("Meridional section · d1 "));
    }

    #[test]
    fn front_view_has_two_outlines_per_blade() {
        let g = reference();
        let d = front_view(&g);
        let closed = d
            .entities
            .iter()
            .filter(|e| matches!(e.shape, Shape::Polyline { closed: true, .. }))
            .count();
        assert_eq!(closed, 2 * g.blade.count as usize);
        let names: Vec<&str> = d.layers().iter().map(|l| l.name).collect();
        assert_eq!(
            names,
            ["CONTOUR", "CONSTRUCTION", "BLADE-HUB", "BLADE-SHROUD"]
        );
    }

    #[test]
    fn layer_weights_are_standard_dxf_values() {
        const STANDARD: [i16; 24] = [
            0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140,
            158, 200, 211,
        ];
        for layer in [
            CONTOUR,
            AXIS,
            CONSTRUCTION,
            STREAMLINES,
            BLADE_EDGES,
            BLADE_HUB,
            BLADE_SHROUD,
            TEXT,
        ] {
            assert!(STANDARD.contains(&layer.weight), "{}", layer.name);
        }
    }
}
