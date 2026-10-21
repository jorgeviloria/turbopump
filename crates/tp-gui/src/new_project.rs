//! The "New project" assistant, in three steps: the machine and the project data, the
//! global setup (design point, fluid and inlet), and the components the project starts
//! with. Beside the global setup, the values that follow from it: specific speed,
//! impeller shape, specific energy and power.

use std::f64::consts::PI;

use eframe::egui::{self, Align, ComboBox, Layout, Modal, RichText, Ui, vec2};
use tp_project::{
    ComponentKind, DesignPoint, EnergyTransfer, FlowRate, GlobalSetup, MachineType, Project,
    ProjectError, Rotation,
};
use tp_units::{DisplayUnits, QuantityKind, STANDARD_GRAVITY, SpecificSpeed};

use crate::dialogs::{header, primary, secondary};
use crate::i18n::{tf, tr};
use crate::icons;
use crate::theme::{Palette, bold, component_color, icon};
use crate::widgets::{self, label, props, value_info, value_row};

/// The steps of the assistant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Machine,
    Setup,
    Components,
}

impl Step {
    const ALL: [Step; 3] = [Step::Machine, Step::Setup, Step::Components];

    fn title(self) -> &'static str {
        match self {
            Step::Machine => tr("Machine"),
            Step::Setup => tr("Global setup"),
            Step::Components => tr("Components"),
        }
    }

    fn index(self) -> usize {
        Step::ALL.iter().position(|&s| s == self).unwrap_or(0)
    }
}

/// What follows the impeller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Diffuser {
    None,
    Vaneless,
    Vaned,
}

/// Inputs of the assistant, in SI.
#[derive(Debug, Clone)]
pub(crate) struct NewProjectForm {
    step: Step,
    name: String,
    author: String,
    description: String,
    flow: f64,
    /// The flow is given as a mass flow.
    mass_flow: bool,
    /// Head in m, or total pressure rise in Pa when `pressure` is set.
    energy: f64,
    pressure: bool,
    speed: f64,
    rotation: Rotation,
    fluid: String,
    inlet_pressure: f64,
    inlet_temperature: f64,
    diffuser: Diffuser,
    volute: bool,
    /// The liquid for the fluid and inlet state it was computed for.
    liquid: Option<(
        String,
        f64,
        f64,
        Result<tp_fluids::LiquidProperties, String>,
    )>,
}

impl Default for NewProjectForm {
    fn default() -> Self {
        let setup = GlobalSetup::new(
            MachineType::Pump,
            DesignPoint {
                flow: FlowRate::Volume(100.0 / 3600.0),
                energy: EnergyTransfer::Head(32.0),
                speed: 2900.0 * PI / 30.0,
            },
        );
        Self {
            step: Step::Machine,
            name: tr("Untitled").into(),
            author: String::new(),
            description: String::new(),
            flow: 100.0 / 3600.0,
            mass_flow: false,
            energy: 32.0,
            pressure: false,
            speed: setup.design_point.speed,
            rotation: setup.rotation,
            fluid: setup.fluid,
            inlet_pressure: setup.inlet.total_pressure,
            inlet_temperature: setup.inlet.total_temperature,
            diffuser: Diffuser::None,
            volute: true,
            liquid: None,
        }
    }
}

/// What the assistant asks for in one frame.
pub(crate) enum Outcome {
    Open,
    Cancel,
    Create(Box<Project>),
}

impl NewProjectForm {
    /// The liquid at the inlet, computed again only when the fluid or the state change.
    fn liquid(&mut self) -> Result<&tp_fluids::LiquidProperties, &str> {
        let key = (
            self.fluid.trim().to_owned(),
            self.inlet_pressure,
            self.inlet_temperature,
        );
        let stale = self
            .liquid
            .as_ref()
            .is_none_or(|(f, p, t, _)| (f, *p, *t) != (&key.0, key.1, key.2));
        if stale {
            let result =
                tp_fluids::liquid_properties(&key.0, key.1, key.2).map_err(|e| e.to_string());
            self.liquid = Some((key.0, key.1, key.2, result));
        }
        match &self.liquid {
            Some((.., Ok(l))) => Ok(l),
            Some((.., Err(e))) => Err(e),
            None => Err(""),
        }
    }

    /// Density of the liquid, or that of water when it is unknown.
    fn density(&mut self) -> f64 {
        self.liquid().map_or(1000.0, |l| l.density)
    }

    /// The volume flow, m³/s, and the head, m.
    fn flow_and_head(&mut self) -> (f64, f64) {
        let rho = self.density();
        let q = if self.mass_flow {
            self.flow / rho
        } else {
            self.flow
        };
        let h = if self.pressure {
            self.energy / (rho * STANDARD_GRAVITY)
        } else {
            self.energy
        };
        (q, h)
    }

    /// The project the assistant describes.
    fn build(&self) -> Result<Project, ProjectError> {
        let mut setup = GlobalSetup::new(
            MachineType::Pump,
            DesignPoint {
                flow: if self.mass_flow {
                    FlowRate::Mass(self.flow)
                } else {
                    FlowRate::Volume(self.flow)
                },
                energy: if self.pressure {
                    EnergyTransfer::TotalPressureDifference(self.energy)
                } else {
                    EnergyTransfer::Head(self.energy)
                },
                speed: self.speed,
            },
        );
        setup.fluid = self.fluid.trim().to_owned();
        setup.rotation = self.rotation;
        setup.inlet.total_pressure = self.inlet_pressure;
        setup.inlet.total_temperature = self.inlet_temperature;
        let mut project = Project::new(self.name.trim(), MachineType::Pump, setup)?;
        project.info.author = self.author.trim().to_owned();
        project.info.description = self.description.trim().to_owned();
        project.add_component(ComponentKind::Impeller, None)?;
        match self.diffuser {
            Diffuser::None => {}
            Diffuser::Vaneless => {
                project.add_component(ComponentKind::Stator { vaned: false }, None)?;
            }
            Diffuser::Vaned => {
                project.add_component(ComponentKind::Stator { vaned: true }, None)?;
            }
        }
        if self.volute {
            project.add_component(ComponentKind::Volute, None)?;
        }
        Ok(project)
    }

    /// Draws the assistant. `error` reports a project that could not be built.
    pub(crate) fn show(
        &mut self,
        ctx: &egui::Context,
        units: &DisplayUnits,
        error: &mut dyn FnMut(String),
    ) -> Outcome {
        let mut outcome = Outcome::Open;
        let response = Modal::new(egui::Id::new("new_project")).show(ctx, |ui| {
            ui.set_width(760.0);
            header(
                ui,
                |ui| {
                    let (rect, _) = ui.allocate_exact_size(vec2(44.0, 44.0), egui::Sense::hover());
                    icons::component(
                        ui.painter(),
                        rect,
                        ComponentKind::Impeller,
                        component_color(ComponentKind::Impeller),
                    );
                },
                tr("New project"),
                &tf(
                    "Step {} of {} · {}",
                    &[
                        &(self.step.index() + 1),
                        &Step::ALL.len(),
                        &self.step.title(),
                    ],
                ),
            );
            self.steps_bar(ui);
            ui.separator();
            ui.allocate_ui_with_layout(
                vec2(ui.available_width(), 360.0),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_min_height(360.0);
                    match self.step {
                        Step::Machine => self.machine(ui),
                        Step::Setup => self.setup(ui, units),
                        Step::Components => self.components(ui),
                    }
                },
            );
            ui.separator();
            ui.horizontal(|ui| {
                if secondary(ui, tr("Cancel")) {
                    outcome = Outcome::Cancel;
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if primary(ui, tr("Create")) {
                        match self.build() {
                            Ok(project) => outcome = Outcome::Create(Box::new(project)),
                            Err(e) => error(tf(
                                "Invalid project: {}",
                                &[&crate::i18n::message(&e.to_string())],
                            )),
                        }
                    }
                    let index = self.step.index();
                    if index + 1 < Step::ALL.len()
                        && secondary(ui, &format!("{}  {}", tr("Next"), icon::CARET_RIGHT))
                    {
                        self.step = Step::ALL[index + 1];
                    }
                    if index > 0 && secondary(ui, &format!("{}  {}", icon::CARET_LEFT, tr("Back")))
                    {
                        self.step = Step::ALL[index - 1];
                    }
                });
            });
        });
        if matches!(outcome, Outcome::Open) && response.should_close() {
            outcome = Outcome::Cancel;
        }
        outcome
    }

    /// The steps, the current one highlighted; a click goes to any of them.
    fn steps_bar(&mut self, ui: &mut Ui) {
        let p = Palette::of(ui);
        ui.horizontal(|ui| {
            for (k, step) in Step::ALL.into_iter().enumerate() {
                if k > 0 {
                    ui.label(RichText::new(icon::CARET_RIGHT).color(p.weak));
                }
                let current = step == self.step;
                let done = step.index() < self.step.index();
                let mark = if done {
                    icon::CHECK.to_owned()
                } else {
                    (k + 1).to_string()
                };
                let text = RichText::new(format!("{mark}  {}", step.title()));
                let text = if current {
                    text.strong()
                } else {
                    text.color(p.weak)
                };
                if ui.add(egui::Button::selectable(current, text)).clicked() {
                    self.step = step;
                }
            }
        });
        ui.add_space(4.0);
    }

    fn machine(&mut self, ui: &mut Ui) {
        crate::steps::section(ui, tr("Machine type"));
        let p = Palette::of(ui);
        let machines: [(&str, &str, &str, bool); 5] = [
            (
                "",
                tr("Pump"),
                tr("Centrifugal pump: impeller, diffuser and volute"),
                true,
            ),
            (icon::FAN, tr("Fan"), tr("Planned for phase 6"), false),
            (
                icon::WIND,
                tr("Compressor"),
                tr("Planned for phase 6"),
                false,
            ),
            (
                icon::ENGINE,
                tr("Gas turbine"),
                tr("Planned for phase 6"),
                false,
            ),
            (
                icon::DROP,
                tr("Hydro turbine"),
                tr("Planned for phase 6"),
                false,
            ),
        ];
        ui.horizontal_wrapped(|ui| {
            for (glyph, name, note, available) in machines {
                let size = vec2(138.0, 92.0);
                let (rect, response) = ui.allocate_exact_size(
                    size,
                    if available {
                        egui::Sense::click()
                    } else {
                        egui::Sense::hover()
                    },
                );
                let selected = available;
                let painter = ui.painter();
                let fill = if selected {
                    p.selected
                } else if response.hovered() && available {
                    p.hover
                } else {
                    egui::Color32::TRANSPARENT
                };
                let stroke = if selected { p.accent } else { p.border };
                painter.rect(
                    rect,
                    6,
                    fill,
                    egui::Stroke::new(1.0, stroke),
                    egui::StrokeKind::Inside,
                );
                let ink = if available { p.text } else { p.weak };
                let icon_rect = egui::Rect::from_center_size(
                    rect.center_top() + vec2(0.0, 26.0),
                    vec2(30.0, 30.0),
                );
                if glyph.is_empty() {
                    icons::component(
                        painter,
                        icon_rect,
                        ComponentKind::Impeller,
                        component_color(ComponentKind::Impeller),
                    );
                } else {
                    painter.text(
                        icon_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        glyph,
                        egui::FontId::proportional(26.0),
                        ink,
                    );
                }
                painter.text(
                    rect.center_top() + vec2(0.0, 56.0),
                    egui::Align2::CENTER_CENTER,
                    name,
                    egui::FontId::new(14.0, crate::theme::bold_family()),
                    ink,
                );
                if !available {
                    painter.text(
                        rect.center_top() + vec2(0.0, 75.0),
                        egui::Align2::CENTER_CENTER,
                        tr("coming later"),
                        egui::FontId::proportional(11.0),
                        p.weak,
                    );
                }
                response.on_hover_text(note);
            }
        });

        crate::steps::section(ui, tr("Project"));
        props(ui, "new_project_info", |ui| {
            label(ui, tr("Name"), "");
            widgets::text_field(ui, &mut self.name);
            ui.end_row();
            label(ui, tr("Author"), "");
            widgets::text_field(ui, &mut self.author);
            ui.end_row();
            label(
                ui,
                tr("Description"),
                tr("Optional: what the project is for"),
            );
            widgets::text_field(ui, &mut self.description);
            ui.end_row();
        });
    }

    fn setup(&mut self, ui: &mut Ui, units: &DisplayUnits) {
        ui.columns(2, |columns| {
            let ui = &mut columns[0];
            crate::steps::section(ui, tr("Design point"));
            props(ui, "new_project_point", |ui| {
                label(ui, tr("Flow rate"), "");
                ui.horizontal(|ui| {
                    widgets::halves(ui);
                    let kind = if self.mass_flow {
                        QuantityKind::MassFlow
                    } else {
                        QuantityKind::VolumeFlow
                    };
                    widgets::quantity(ui, &mut self.flow, kind, units, 1e-6..=1e5);
                    let before = self.mass_flow;
                    ComboBox::from_id_salt("new_flow_kind")
                        .selected_text(tr(if self.mass_flow { "mass" } else { "volume" }))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.mass_flow, false, tr("volume"));
                            ui.selectable_value(&mut self.mass_flow, true, tr("mass"));
                        });
                    if self.mass_flow != before {
                        // The same flow, given the other way.
                        let rho = self.density();
                        self.flow = if self.mass_flow {
                            self.flow * rho
                        } else {
                            self.flow / rho
                        };
                    }
                });
                ui.end_row();
                label(ui, tr("Energy"), "");
                ui.horizontal(|ui| {
                    widgets::halves(ui);
                    let kind = if self.pressure {
                        QuantityKind::Pressure
                    } else {
                        QuantityKind::Head
                    };
                    widgets::quantity(ui, &mut self.energy, kind, units, 1e-3..=1e9);
                    let before = self.pressure;
                    ComboBox::from_id_salt("new_energy_kind")
                        .selected_text(tr(if self.pressure {
                            "pressure rise"
                        } else {
                            "head"
                        }))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.pressure, false, tr("head"));
                            ui.selectable_value(&mut self.pressure, true, tr("pressure rise"));
                        });
                    if self.pressure != before {
                        let rho_g = self.density() * STANDARD_GRAVITY;
                        self.energy = if self.pressure {
                            self.energy * rho_g
                        } else {
                            self.energy / rho_g
                        };
                    }
                });
                ui.end_row();
                label(ui, tr("Rotational speed"), "");
                widgets::quantity(
                    ui,
                    &mut self.speed,
                    QuantityKind::RotationalSpeed,
                    units,
                    0.1..=1e5,
                );
                ui.end_row();
                label(
                    ui,
                    tr("Rotation"),
                    tr("Seen from the inlet, looking downstream"),
                );
                let text = |r: Rotation| {
                    tr(match r {
                        Rotation::Clockwise => "clockwise",
                        Rotation::CounterClockwise => "counter-clockwise",
                    })
                };
                ComboBox::from_id_salt("new_rotation")
                    .selected_text(text(self.rotation))
                    .show_ui(ui, |ui| {
                        for r in [Rotation::Clockwise, Rotation::CounterClockwise] {
                            ui.selectable_value(&mut self.rotation, r, text(r));
                        }
                    });
                ui.end_row();
            });
            crate::steps::section(ui, tr("Fluid and inlet conditions"));
            props(ui, "new_project_fluid", |ui| {
                label(
                    ui,
                    tr("Fluid"),
                    tr("A CoolProp name; water also works without CoolProp"),
                );
                ui.horizontal(|ui| {
                    widgets::halves(ui);
                    widgets::text_field(ui, &mut self.fluid);
                    ComboBox::from_id_salt("new_common_fluids")
                        .selected_text(tr("common…"))
                        .show_ui(ui, |ui| {
                            for name in crate::steps::COMMON_FLUIDS {
                                if ui.selectable_label(self.fluid == *name, *name).clicked() {
                                    self.fluid = (*name).to_owned();
                                }
                            }
                        });
                });
                ui.end_row();
                label(ui, tr("Inlet total pressure"), "");
                widgets::quantity(
                    ui,
                    &mut self.inlet_pressure,
                    QuantityKind::Pressure,
                    units,
                    1.0..=1e9,
                );
                ui.end_row();
                label(ui, tr("Inlet total temperature"), "");
                widgets::quantity(
                    ui,
                    &mut self.inlet_temperature,
                    QuantityKind::Temperature,
                    units,
                    1.0..=2000.0,
                );
                ui.end_row();
            });

            let ui = &mut columns[1];
            self.values(ui, units);
        });
    }

    /// The values that follow from the global setup.
    fn values(&mut self, ui: &mut Ui, units: &DisplayUnits) {
        use QuantityKind::*;
        let p = Palette::of(ui);
        crate::steps::section(ui, tr("Values"));
        let (q, h) = self.flow_and_head();
        let liquid = self.liquid().map(|l| l.density).map_err(str::to_owned);
        let nq = SpecificSpeed::from_head(q, h, self.speed).nq();
        let (shape, note) = if nq < 70.0 {
            (tr("radial"), tr("Usual for nq below about 70"))
        } else if nq < 160.0 {
            (tr("mixed flow"), tr("Usual for nq from about 70 to 160"))
        } else {
            (tr("axial"), tr("Usual for nq above about 160"))
        };
        props(ui, "new_project_values", |ui| {
            value_info(
                ui,
                tr("Specific speed nq"),
                tr("n·√Q / H^¾ with n in rpm, Q in m³/s and H in m"),
                format!("{nq:.1}"),
            );
            value_info(ui, tr("Impeller shape"), note, shape);
            value_info(
                ui,
                tr("Specific energy Y"),
                tr("Y = g·H"),
                units.show(SpecificEnergy, STANDARD_GRAVITY * h),
            );
            match &liquid {
                Ok(rho) => {
                    value_row(ui, tr("Density ρ"), units.show(Density, *rho));
                    value_row(ui, tr("Mass flow"), units.show(MassFlow, rho * q));
                    value_info(
                        ui,
                        tr("Hydraulic power"),
                        tr("ρ·g·Q·H, the power the impeller gives the liquid"),
                        units.show(Power, rho * STANDARD_GRAVITY * q * h),
                    );
                }
                Err(_) => value_row(ui, tr("Density ρ"), "—"),
            }
        });
        if let Err(e) = &liquid {
            ui.add_space(6.0);
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
        if nq >= 70.0 {
            ui.add_space(6.0);
            ui.label(
                RichText::new(tr(
                    "The design code works out radial impellers; above nq 70 check the results with care.",
                ))
                .color(p.weak),
            );
        }
    }

    fn components(&mut self, ui: &mut Ui) {
        let p = Palette::of(ui);
        crate::steps::section(ui, tr("Components"));
        ui.label(
            RichText::new(tr(
                "The project starts with these components, in the order the flow meets them. \
                 More can be added later from the toolbar.",
            ))
            .color(p.weak),
        );
        ui.add_space(6.0);
        props(ui, "new_project_components", |ui| {
            label(ui, tr("Impeller"), tr("Every pump starts with one"));
            widgets::value(ui, tr("included"));
            ui.end_row();
            label(
                ui,
                tr("Diffuser"),
                tr("Between the impeller and the volute"),
            );
            let text = |d: Diffuser| {
                tr(match d {
                    Diffuser::None => "none",
                    Diffuser::Vaneless => "vaneless",
                    Diffuser::Vaned => "vaned",
                })
            };
            ComboBox::from_id_salt("new_diffuser")
                .selected_text(text(self.diffuser))
                .show_ui(ui, |ui| {
                    for d in [Diffuser::None, Diffuser::Vaneless, Diffuser::Vaned] {
                        ui.selectable_value(&mut self.diffuser, d, text(d));
                    }
                });
            ui.end_row();
            label(ui, tr("Volute"), tr("Spiral casing that collects the flow"));
            ui.checkbox(&mut self.volute, "");
            ui.end_row();
        });

        // The chain, as the flow meets it.
        ui.add_space(14.0);
        let mut chain = vec![ComponentKind::Impeller];
        match self.diffuser {
            Diffuser::None => {}
            Diffuser::Vaneless => chain.push(ComponentKind::Stator { vaned: false }),
            Diffuser::Vaned => chain.push(ComponentKind::Stator { vaned: true }),
        }
        if self.volute {
            chain.push(ComponentKind::Volute);
        }
        ui.horizontal(|ui| {
            for (k, kind) in chain.iter().enumerate() {
                if k > 0 {
                    ui.label(RichText::new(icon::ARROW_RIGHT).size(18.0).color(p.weak));
                }
                ui.vertical(|ui| {
                    let (rect, _) = ui.allocate_exact_size(vec2(56.0, 56.0), egui::Sense::hover());
                    icons::component(ui.painter(), rect, *kind, component_color(*kind));
                    ui.label(
                        RichText::new(match kind {
                            ComponentKind::Impeller => tr("Impeller"),
                            ComponentKind::Stator { vaned: true } => tr("Vaned diffuser"),
                            ComponentKind::Stator { vaned: false } => tr("Vaneless diffuser"),
                            ComponentKind::Volute => tr("Volute"),
                        })
                        .small(),
                    );
                });
            }
        });
        ui.add_space(10.0);
        ui.label(bold(tr("Next: the impeller's main dimensions")));
        ui.label(
            RichText::new(tr(
                "After creating the project, its steps open one after another in the tree.",
            ))
            .color(p.weak),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chosen_components_follow_the_impeller() {
        let form = NewProjectForm {
            diffuser: Diffuser::Vaned,
            ..NewProjectForm::default()
        };
        let project = form.build().unwrap();
        let kinds: Vec<ComponentKind> = project.components().iter().map(|c| c.kind()).collect();
        assert_eq!(
            kinds,
            [
                ComponentKind::Impeller,
                ComponentKind::Stator { vaned: true },
                ComponentKind::Volute
            ]
        );
    }
}
