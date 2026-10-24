//! The objective strip under the toolbar. It keeps the design point in sight: the
//! target the user set, next to what the design is predicted to give, with the head
//! judged against the tolerance of the project's acceptance class.

use eframe::egui::{self, Color32, FontFamily, FontId, Response, TextFormat, Ui, text::LayoutJob};
use tp_design::acceptance::ClassCriteria;
use tp_project::{ComponentId, DesignStep, EnergyTransfer, FlowRate, Project, StepStatus};
use tp_units::{DisplayUnits, QuantityKind};

use crate::derived::Derived;
use crate::i18n::{component_name, message, tf, tr};
use crate::theme::{Palette, bold_family, icon};

/// What a click on the strip asks for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Action {
    /// Open the design point and fluid.
    Setup,
    /// Show the performance curves.
    Curves,
    /// Open a step.
    Step(ComponentId, DesignStep),
}

/// How much the strip shows, by its width.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Detail {
    Compact,
    Medium,
    Full,
}

const SIZE: f32 = 12.5;

/// One line of text in several colors, plain or bold.
struct Line {
    job: LayoutJob,
}

impl Line {
    fn new() -> Self {
        Self {
            job: LayoutJob::default(),
        }
    }

    fn add(&mut self, text: &str, color: Color32, family: FontFamily, size: f32) -> &mut Self {
        self.job.append(
            text,
            0.0,
            TextFormat::simple(FontId::new(size, family), color),
        );
        self
    }

    fn text(&mut self, text: &str, color: Color32) -> &mut Self {
        self.add(text, color, FontFamily::Proportional, SIZE)
    }

    fn strong(&mut self, text: &str, color: Color32) -> &mut Self {
        self.add(text, color, bold_family(), SIZE)
    }

    /// A small upper-case caption.
    fn caption(&mut self, text: &str, color: Color32) -> &mut Self {
        self.add(text, color, bold_family(), 10.5)
    }

    /// A named value: "H 32.0 m", the number in bold.
    fn value(&mut self, name: &str, value: &str, p: &Palette) -> &mut Self {
        self.text(&format!("{name} "), p.text).strong(value, p.text)
    }

    fn dot(&mut self, p: &Palette) -> &mut Self {
        self.text("  ·  ", p.weak)
    }
}

/// A frameless button holding `line`, framed while hovered. It keeps the widest it has
/// been under `key`, so that a number growing shorter does not pull the chips after it.
fn chip(ui: &mut Ui, key: &str, line: &mut Line, tip: &str) -> Response {
    let job = std::mem::take(&mut line.job);
    let id = egui::Id::new(("objective_chip", key));
    let widest = ui.data(|d| d.get_temp::<f32>(id)).unwrap_or(0.0);
    let response = ui.add(
        egui::Button::new(job)
            .frame_when_inactive(false)
            .min_size(egui::vec2(widest, 0.0)),
    );
    let width = response.rect.width();
    if width > widest {
        ui.data_mut(|d| d.insert_temp(id, width));
    }
    if tip.is_empty() {
        response
    } else {
        response.on_hover_ui(|ui| {
            ui.set_max_width(360.0);
            ui.label(tip);
        })
    }
}

/// The strip. `impeller` is the one whose prediction it shows.
pub(crate) fn show(
    ui: &mut Ui,
    project: &Project,
    derived: &Derived,
    impeller: Option<ComponentId>,
    units: &DisplayUnits,
) -> Option<Action> {
    let p = Palette::of(ui);
    let detail = match ui.available_width() {
        w if w >= 1400.0 => Detail::Full,
        w if w >= 1000.0 => Detail::Medium,
        _ => Detail::Compact,
    };
    let mut action = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        let mut line = target(project, derived, impeller, units, detail, &p);
        let source = match &derived.liquid {
            Some(Ok(l)) => match l.source {
                tp_fluids::PropertySource::CoolProp => tr("fluid properties: CoolProp"),
                tp_fluids::PropertySource::BuiltinWater => {
                    tr("fluid properties: built-in water model")
                }
            },
            Some(Err(_)) => tr("fluid properties: unavailable"),
            None => "",
        };
        let tip = format!(
            "{}\n{source}",
            tr("The design point, the fluid and the inlet conditions: click to edit them")
        );
        if chip(ui, "target", &mut line, &tip).clicked() {
            action = Some(Action::Setup);
        }
        ui.separator();
        if let Some(a) = prediction(ui, project, derived, impeller, units, detail, &p) {
            action = Some(a);
        }
        if let Some(a) = stale(ui, project, &p) {
            action = Some(a);
        }
    });
    action
}

/// What the user asked for: flow, head and speed, then the fluid and nq.
fn target(
    project: &Project,
    derived: &Derived,
    impeller: Option<ComponentId>,
    units: &DisplayUnits,
    detail: Detail,
    p: &Palette,
) -> Line {
    let setup = project.setup();
    let mut line = Line::new();
    line.add(
        &format!("{}  ", icon::TARGET),
        p.accent,
        FontFamily::Proportional,
        14.0,
    )
    .caption(&format!("{}  ", tr("TARGET")), p.weak);
    let duty = impeller.and_then(|id| project.pump_duty(id).ok());
    let flow = match (&duty, setup.design_point.flow) {
        (Some(d), _) => units.show(QuantityKind::VolumeFlow, d.flow),
        (None, FlowRate::Volume(q)) => units.show(QuantityKind::VolumeFlow, q),
        (None, FlowRate::Mass(m)) => units.show(QuantityKind::MassFlow, m),
    };
    line.value("Q", &flow, p).dot(p);
    let energy = match (&duty, setup.design_point.energy) {
        (Some(d), _) => ("H", units.show(QuantityKind::Head, d.machine_head)),
        (None, EnergyTransfer::Head(h)) => ("H", units.show(QuantityKind::Head, h)),
        (None, EnergyTransfer::TotalPressureDifference(dp)) => {
            ("Δp", units.show(QuantityKind::Pressure, dp))
        }
        (None, other) => ("Π", format!("{:.3}", other.value())),
    };
    line.value(energy.0, &energy.1, p);
    if detail == Detail::Compact {
        return line;
    }
    line.dot(p).value(
        "n",
        &units.show(QuantityKind::RotationalSpeed, setup.design_point.speed),
        p,
    );
    line.dot(p).text(
        &format!(
            "{} {}",
            crate::i18n::fluid_name(&setup.fluid),
            units.show(QuantityKind::Temperature, setup.inlet.total_temperature)
        ),
        p.text,
    );
    if detail == Detail::Full
        && let Some(main) = impeller.and_then(|id| derived.main(id))
    {
        line.dot(p).value("nq", &format!("{:.1}", main.nq), p);
    }
    line
}

/// What the design is predicted to give, at the design flow.
fn prediction(
    ui: &mut Ui,
    project: &Project,
    derived: &Derived,
    impeller: Option<ComponentId>,
    units: &DisplayUnits,
    detail: Detail,
    p: &Palette,
) -> Option<Action> {
    let warn = ui.visuals().warn_fg_color;
    let error = ui.visuals().error_fg_color;
    let Some(id) = impeller else {
        ui.label(egui::RichText::new(tr("Add an impeller to see the prediction.")).color(p.weak));
        return None;
    };
    let name = project
        .component(id)
        .map_or_else(String::new, |c| component_name(&c.name));
    let duty = project.pump_duty(id).ok();
    let mut caption = Line::new();
    caption.caption(&tf("PREDICTED · {}", &[&name.to_uppercase()]), p.weak);
    if duty.is_some_and(|d| d.stages > 1) {
        caption.caption(&format!(" {}", tr("PER STAGE")), p.weak);
    }
    chip(
        ui,
        "caption",
        &mut caption,
        tr(
            "One-dimensional model of the performance prediction: main dimensions and collector. Editing the mean lines or the profiles does not change it, and it is not a CFD result.",
        ),
    );
    let results = derived.impeller(id)?;
    let (main, perf) = match (&results.main, &results.performance) {
        (Ok(main), Ok(perf)) => (main, perf),
        (Err(e), _) | (_, Err(e)) => {
            let mut line = Line::new();
            line.text(&format!("{} ", icon::X_CIRCLE), error)
                .text(tr("no prediction"), error);
            chip(ui, "failed", &mut line, &message(e));
            return None;
        }
    };
    let mut action = None;
    let mut curves = |response: Response| {
        if response.clicked() {
            action = Some(Action::Curves);
        }
    };

    // Head, against the target and its tolerance.
    let head = perf.design.head;
    let acceptance = project
        .acceptance
        .resolve(Some(&ClassCriteria::of(main, Some(perf))));
    let mut line = Line::new();
    line.value("H", &units.show(QuantityKind::Head, head), p);
    let mut tip = tr("Predicted head at the design flow.").to_owned();
    if let (Some(duty), Some(acc)) = (duty, acceptance) {
        let within = acc.head_within(head, duty.head);
        let (glyph, color) = if within {
            (icon::CHECK_CIRCLE, p.done)
        } else {
            (icon::WARNING, warn)
        };
        let deviation = (head - duty.head) / duty.head * 100.0;
        line.text(&format!("  {deviation:+.1} % {glyph}"), color);
        tip = tf(
            "Predicted head at the design flow, {}, against the target {}: {} %. Tolerance ±{} % of class {} (Gülich Table 15.2, after ISO 9906), used here by analogy with an acceptance test. Change it under Design point › Acceptance.",
            &[
                &units.show(QuantityKind::Head, head),
                &units.show(QuantityKind::Head, duty.head),
                &format!("{deviation:+.1}"),
                &format!("{:.1}", acc.head_tolerance * 100.0),
                &acc.class.name(),
            ],
        );
    }
    curves(chip(ui, "head", &mut line, &tip));

    // Efficiency, with the scatter of the statistics.
    if detail >= Detail::Medium {
        dot(ui, p);
        let mut line = Line::new();
        line.value("η", &format!("{:.1} %", perf.design.efficiency * 100.0), p);
        line.text(
            &format!(
                "  ({} {:.0} ± {:.0})",
                tr("stat."),
                main.efficiencies.overall * 100.0,
                main.efficiency_tolerance * 100.0
            ),
            p.weak,
        );
        let tip = tf(
            "Efficiency predicted from the losses at the design flow: {} %. The statistics of the main dimensions expect {} ± {} %.",
            &[
                &format!("{:.1}", perf.design.efficiency * 100.0),
                &format!("{:.1}", main.efficiencies.overall * 100.0),
                &format!("{:.1}", main.efficiency_tolerance * 100.0),
            ],
        );
        curves(chip(ui, "efficiency", &mut line, &tip));
    }

    // Power.
    if detail == Detail::Full {
        dot(ui, p);
        let mut line = Line::new();
        line.value("P", &units.show(QuantityKind::Power, perf.design.power), p);
        curves(chip(
            ui,
            "power",
            &mut line,
            tr("Shaft power at the design flow"),
        ));
    }

    // Cavitation: the NPSH₃ against the NPSH available.
    dot(ui, p);
    let enough = perf.npsh_available > perf.design.npsh_required;
    let (glyph, color) = if enough {
        (icon::CHECK_CIRCLE, p.done)
    } else {
        (icon::X_CIRCLE, error)
    };
    let mut line = Line::new();
    if detail == Detail::Compact {
        line.text("NPSH ", p.text);
    } else {
        line.value(
            "NPSH₃",
            &units.show(QuantityKind::Head, perf.design.npsh_required),
            p,
        )
        .text(" / ", p.weak)
        .value(
            "NPSHa",
            &units.show(QuantityKind::Head, perf.npsh_available),
            p,
        );
    }
    line.text(&format!(" {glyph}"), color);
    let tip = if enough {
        tr("The NPSH₃ at the design flow is below the NPSH available of the inlet conditions.")
    } else {
        tr(
            "The NPSH₃ at the design flow reaches the NPSH available of the inlet conditions: the pump cavitates.",
        )
    };
    curves(chip(ui, "npsh", &mut line, tip));

    // Where the best efficiency is.
    if detail == Detail::Full && perf.design.flow > 0.0 {
        dot(ui, p);
        let mut line = Line::new();
        line.text(&format!("{} ", tr("best at")), p.text)
            .strong(&format!("{:.2}", perf.best.flow / perf.design.flow), p.text)
            .text(" Q", p.text);
        curves(chip(
            ui,
            "best",
            &mut line,
            tr("Flow of the best efficiency over the design flow"),
        ));
    }
    action
}

fn dot(ui: &mut Ui, p: &Palette) {
    ui.label(egui::RichText::new("·").color(p.weak));
}

/// The steps an earlier change left outdated, with a way to the first of them.
fn stale(ui: &mut Ui, project: &Project, p: &Palette) -> Option<Action> {
    let mut first = None;
    let mut count = 0;
    for component in project.components() {
        for (step, status) in component.steps() {
            if status == StepStatus::Stale {
                count += 1;
                first.get_or_insert((component.id(), step));
            }
        }
    }
    let (id, step) = first?;
    ui.add_space(6.0);
    let text = if count == 1 {
        tr("1 step to review").to_owned()
    } else {
        tf("{} steps to review", &[&count])
    };
    let mut line = Line::new();
    line.text(&format!("{}  {text}", icon::ARROWS_CLOCKWISE), p.stale);
    chip(
        ui,
        "stale",
        &mut line,
        tr("An earlier change left these steps outdated: click to open the first"),
    )
    .clicked()
    .then_some(Action::Step(id, step))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A number growing shorter leaves its chip as wide as it was, so the chips after it
    /// stay put.
    #[test]
    fn a_chip_keeps_the_widest_it_has_been() {
        let ctx = egui::Context::default();
        let width = |text: &str| {
            let mut width = 0.0;
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                let mut line = Line::new();
                line.text(text, Color32::WHITE);
                width = chip(ui, "test", &mut line, "").rect.width();
            });
            output.textures_delta.clear();
            width
        };
        let long = width("H 32.32 m  +15.4 %");
        let short = width("H 32.32 m  +1.0 %");
        assert!(long > 0.0);
        assert_eq!(short, long);
    }
}
