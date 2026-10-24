//! Icons: component pictograms drawn with the painter, and the glyph of each design
//! step.

use std::f32::consts::TAU;

use eframe::egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2};
use tp_project::{ComponentKind, DesignStep};

use crate::theme::icon;

/// Glyph of a design step.
pub(crate) fn step(step: DesignStep) -> &'static str {
    use DesignStep::*;
    match step {
        DuctDimensions => icon::PIPE,
        MainDimensions => icon::RULER,
        MeridionalContour => icon::BEZIER_CURVE,
        BladeProperties => icon::SLIDERS_HORIZONTAL,
        BladeMeanLines => icon::PATH,
        BladeProfiles => icon::WAVE_SINE,
        BladeEdges => icon::POLYGON,
        VoluteSetup => icon::GEAR,
        CrossSection => icon::CIRCLE_HALF,
        SpiralDevelopment => icon::SPIRAL,
        Diffuser => icon::ARROWS_OUT,
        CutWater => icon::ANGLE,
        WallThickness => icon::SQUARE_HALF,
        CfdSetup => icon::WIND,
        ModelSettings => icon::CUBE,
        ModelFinishing => icon::WRENCH,
    }
}

fn polar(c: Pos2, r: f32, angle: f32) -> Pos2 {
    pos2(c.x + r * angle.cos(), c.y - r * angle.sin())
}

/// Draws the pictogram of `kind` inside `rect` in `color`.
pub(crate) fn component(painter: &Painter, rect: Rect, kind: ComponentKind, color: Color32) {
    let c = rect.center();
    let r = 0.46 * rect.width().min(rect.height());
    let width = (r * 0.09).max(1.2);
    let stroke = Stroke::new(width, color);
    match kind {
        ComponentKind::Impeller => {
            painter.circle_stroke(c, r, stroke);
            painter.circle_filled(c, r * 0.22, color);
            // Backward-swept blades: the angle falls behind as the radius grows.
            for k in 0..6 {
                let start = k as f32 * TAU / 6.0;
                let points: Vec<Pos2> = (0..=8)
                    .map(|i| {
                        let t = i as f32 / 8.0;
                        polar(c, r * (0.3 + 0.62 * t), start - 1.1 * t)
                    })
                    .collect();
                painter.add(Shape::line(points, stroke));
            }
        }
        ComponentKind::Stator { vaned } => {
            painter.circle_stroke(c, r, stroke);
            painter.circle_stroke(c, r * 0.58, stroke);
            if vaned {
                for k in 0..8 {
                    let a = k as f32 * TAU / 8.0;
                    painter.line_segment(
                        [polar(c, r * 0.62, a), polar(c, r * 0.94, a + 0.45)],
                        stroke,
                    );
                }
            } else {
                painter.circle_stroke(c, r * 0.79, Stroke::new(width * 0.6, color));
            }
        }
        ComponentKind::Volute => {
            // A spiral growing round the impeller circle, then the discharge nozzle.
            let inner = r * 0.42;
            let points: Vec<Pos2> = (0..=40)
                .map(|i| {
                    let t = i as f32 / 40.0;
                    polar(c, inner + r * 0.42 * t, -TAU * t + 0.6)
                })
                .collect();
            let end = *points.last().expect("the spiral has points");
            painter.add(Shape::line(points, stroke));
            painter.circle_stroke(c, inner * 0.62, Stroke::new(width * 0.7, color));
            let cutwater = polar(c, inner, 0.6);
            let up = pos2(end.x, rect.top() + r * 0.05);
            painter.line_segment([end, up], stroke);
            painter.line_segment(
                [cutwater, pos2(cutwater.x + r * 0.1, rect.top() + r * 0.05)],
                stroke,
            );
        }
        ComponentKind::InletDuct => {
            // A pipe seen from the side, open at both ends, with the flow along it.
            let (left, right) = (c.x - r * 0.9, c.x + r * 0.9);
            let half = r * 0.48;
            for y in [c.y - half, c.y + half] {
                painter.line_segment([pos2(left, y), pos2(right, y)], stroke);
            }
            for x in [left, right] {
                painter.line_segment([pos2(x, c.y - half), pos2(x, c.y + half)], stroke);
            }
            let tip = pos2(c.x + r * 0.42, c.y);
            painter.line_segment([pos2(c.x - r * 0.42, c.y), tip], stroke);
            for dy in [-1.0, 1.0] {
                painter.line_segment([tip, pos2(tip.x - r * 0.22, c.y + dy * r * 0.2)], stroke);
            }
        }
    }
}
