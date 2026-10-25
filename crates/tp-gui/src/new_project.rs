//! The "New project" assistant, laid out like the settings window: the pages on the
//! left (project, design point, fluid and components) and, on the right, the page in
//! the property rows of the rest of the editor, with the values that follow from what
//! was entered and a summary before creating the project.

use std::f64::consts::PI;

use eframe::egui::{self, Align, ComboBox, Layout, Modal, RichText, Ui, vec2};
use tp_project::{
    ComponentKind, DesignPoint, EnergyTransfer, FlowRate, GlobalSetup, MachineType, Project,
    ProjectError, Rotation,
};
use tp_units::{DisplayUnits, QuantityKind, STANDARD_GRAVITY, SpecificSpeed};

use crate::dialogs::{primary, secondary};
use crate::i18n::{tf, tr};
use crate::theme::{Palette, bold, icon};
use crate::widgets::{self, label, props, value_info, value_row};

/// The pages of the assistant, in the order they are filled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Project,
    DesignPoint,
    Fluid,
    Components,
}

impl Page {
    const ALL: [Page; 4] = [
        Page::Project,
        Page::DesignPoint,
        Page::Fluid,
        Page::Components,
    ];

    fn title(self) -> &'static str {
        match self {
            Page::Project => tr("Project"),
            Page::DesignPoint => tr("Design point"),
            Page::Fluid => tr("Fluid and inlet conditions"),
            Page::Components => tr("Components"),
        }
    }

    /// The name in the list of pages, short enough for one line.
    fn short(self) -> &'static str {
        match self {
            Page::Fluid => tr("Fluid"),
            page => page.title(),
        }
    }

    /// What the page sets, under its title.
    fn summary(self) -> &'static str {
        match self {
            Page::Project => tr("The name and the kind of machine."),
            Page::DesignPoint => {
                tr("The duty the pump is designed for, shared by every component.")
            }
            Page::Fluid => tr("The liquid and its state at the pump inlet."),
            Page::Components => tr("What the project starts with, in the order of the flow."),
        }
    }

    fn glyph(self) -> &'static str {
        match self {
            Page::Project => icon::FILE_PLUS,
            Page::DesignPoint => icon::GAUGE,
            Page::Fluid => icon::DROP,
            Page::Components => icon::SQUARES_FOUR,
        }
    }

    fn index(self) -> usize {
        Page::ALL.iter().position(|&p| p == self).unwrap_or(0)
    }
}

/// Sizes of the window: the list of pages, the gap after it and a page.
const NAV_WIDTH: f32 = 190.0;
const GAP: f32 = 17.0;
const PAGE_WIDTH: f32 = 520.0;
const PAGE_HEIGHT: f32 = 400.0;

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
    page: Page,
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
            page: Page::Project,
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
            // A fixed size, so the window keeps its place from page to page.
            ui.set_width(NAV_WIDTH + GAP + PAGE_WIDTH);
            ui.horizontal(|ui| {
                ui.heading(format!("{}  {}", icon::FILE_PLUS, tr("New project")));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add(egui::Button::new(icon::X).frame(false))
                        .on_hover_text(tr("Cancel"))
                        .clicked()
                    {
                        outcome = Outcome::Cancel;
                    }
                });
            });
            ui.add_space(4.0);
            ui.separator();
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let size = vec2(NAV_WIDTH, PAGE_HEIGHT);
                let nav = ui.allocate_ui_with_layout(size, Layout::top_down(Align::Min), |ui| {
                    ui.set_min_size(size);
                    self.navigation(ui);
                });
                let rect = nav.response.rect;
                ui.painter().vline(
                    rect.right() + GAP / 2.0,
                    rect.y_range(),
                    ui.visuals().widgets.noninteractive.bg_stroke,
                );
                ui.add_space(GAP);
                let size = vec2(PAGE_WIDTH, PAGE_HEIGHT);
                ui.allocate_ui_with_layout(size, Layout::top_down(Align::Min), |ui| {
                    ui.set_min_size(size);
                    egui::ScrollArea::vertical()
                        .id_salt(("new_project_page", self.page.index()))
                        .auto_shrink(false)
                        .show(ui, |ui| {
                            let p = Palette::of(ui);
                            ui.label(bold(self.page.title()).size(16.0));
                            ui.label(RichText::new(self.page.summary()).color(p.weak));
                            ui.add_space(4.0);
                            match self.page {
                                Page::Project => self.project_page(ui),
                                Page::DesignPoint => self.design_point_page(ui, units),
                                Page::Fluid => self.fluid_page(ui, units),
                                Page::Components => self.components_page(ui, units),
                            }
                        });
                });
            });
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
                    let index = self.page.index();
                    if index + 1 < Page::ALL.len()
                        && secondary(ui, &format!("{}  {}", tr("Next"), icon::CARET_RIGHT))
                    {
                        self.page = Page::ALL[index + 1];
                    }
                    if index > 0 && secondary(ui, &format!("{}  {}", icon::CARET_LEFT, tr("Back")))
                    {
                        self.page = Page::ALL[index - 1];
                    }
                });
            });
        });
        if matches!(outcome, Outcome::Open) && response.should_close() {
            outcome = Outcome::Cancel;
        }
        outcome
    }

    /// The pages, numbered; the current one highlighted, the ones before it ticked.
    fn navigation(&mut self, ui: &mut Ui) {
        let p = Palette::of(ui);
        ui.spacing_mut().item_spacing.y = 2.0;
        for page in Page::ALL {
            let done = page.index() < self.page.index();
            let mark = if done {
                icon::CHECK_CIRCLE
            } else {
                page.glyph()
            };
            let label = format!("{mark}  {}", page.short());
            let button = egui::Button::selectable(self.page == page, label)
                .right_text("")
                .min_size(vec2(NAV_WIDTH, 30.0));
            if ui.add(button).clicked() {
                self.page = page;
            }
        }
        ui.add_space(12.0);
        ui.label(
            RichText::new(tf(
                "Step {} of {}",
                &[&(self.page.index() + 1), &Page::ALL.len()],
            ))
            .color(p.weak)
            .small(),
        );
    }

    fn project_page(&mut self, ui: &mut Ui) {
        crate::steps::section(ui, tr("Identification"));
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
        crate::steps::section(ui, tr("Machine"));
        props(ui, "new_project_machine", |ui| {
            value_info(
                ui,
                tr("Machine type"),
                tr("Fans, compressors and turbines arrive in phase 6"),
                tr("Centrifugal pump"),
            );
        });
    }

    fn design_point_page(&mut self, ui: &mut Ui, units: &DisplayUnits) {
        crate::steps::section(ui, tr("Duty"));
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

        use QuantityKind::*;
        let (q, h) = self.flow_and_head();
        let nq = SpecificSpeed::from_head(q, h, self.speed).nq();
        let (shape, note) = if nq < 70.0 {
            (tr("radial"), tr("Usual for nq below about 70"))
        } else if nq < 160.0 {
            (tr("mixed flow"), tr("Usual for nq from about 70 to 160"))
        } else {
            (tr("axial"), tr("Usual for nq above about 160"))
        };
        let power = self
            .liquid()
            .ok()
            .map(|l| l.density * STANDARD_GRAVITY * q * h);
        crate::steps::section_info(
            ui,
            tr("Derived values"),
            if nq >= 70.0 {
                tr(
                    "The design code works out radial impellers; above nq 70 check the results with care.",
                )
            } else {
                ""
            },
        );
        props(ui, "new_project_derived", |ui| {
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
            value_info(
                ui,
                tr("Hydraulic power"),
                tr("ρ·g·Q·H, the power the impeller gives the liquid"),
                power.map_or_else(|| "—".to_owned(), |w| units.show(Power, w)),
            );
        });
    }

    fn fluid_page(&mut self, ui: &mut Ui, units: &DisplayUnits) {
        use QuantityKind::*;
        crate::steps::section(ui, tr("Liquid"));
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
                            if ui
                                .selectable_label(
                                    self.fluid == *name,
                                    crate::i18n::fluid_name(name),
                                )
                                .on_hover_text(crate::i18n::tf("CoolProp name: {}", &[name]))
                                .clicked()
                            {
                                self.fluid = (*name).to_owned();
                            }
                        }
                    });
            });
            ui.end_row();
            label(ui, tr("Inlet total pressure"), "");
            widgets::quantity(ui, &mut self.inlet_pressure, Pressure, units, 1.0..=1e9);
            ui.end_row();
            label(ui, tr("Inlet total temperature"), "");
            widgets::quantity(
                ui,
                &mut self.inlet_temperature,
                Temperature,
                units,
                1.0..=2000.0,
            );
            ui.end_row();
        });
        let (q, _) = self.flow_and_head();
        let liquid = self.liquid().copied().map_err(str::to_owned);
        crate::steps::section(ui, tr("Properties at the inlet"));
        match liquid {
            Ok(l) => props(ui, "new_project_liquid", |ui| {
                value_row(ui, tr("Density ρ"), units.show(Density, l.density));
                value_row(
                    ui,
                    tr("Kinematic viscosity ν"),
                    units.show(KinematicViscosity, l.kinematic_viscosity()),
                );
                value_row(
                    ui,
                    tr("Vapour pressure p_v"),
                    units.show(Pressure, l.vapor_pressure),
                );
                value_row(ui, tr("Mass flow"), units.show(MassFlow, l.density * q));
                let source = match l.source {
                    tp_fluids::PropertySource::CoolProp => "CoolProp",
                    tp_fluids::PropertySource::BuiltinWater => tr("built-in water model (IAPWS)"),
                };
                value_row(ui, tr("Source"), source);
            }),
            Err(e) => crate::messages::card(ui, crate::messages::Severity::Error, &e),
        }
    }

    fn components_page(&mut self, ui: &mut Ui, units: &DisplayUnits) {
        crate::steps::section(ui, tr("Components"));
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

        // The whole project at a glance before creating it.
        let mut chain = vec![tr("Impeller")];
        match self.diffuser {
            Diffuser::None => {}
            Diffuser::Vaneless => chain.push(tr("Vaneless diffuser")),
            Diffuser::Vaned => chain.push(tr("Vaned diffuser")),
        }
        if self.volute {
            chain.push(tr("Volute"));
        }
        let (q, h) = self.flow_and_head();
        let point = format!(
            "{} · {} · {}",
            units.show(QuantityKind::VolumeFlow, q),
            units.show(QuantityKind::Head, h),
            units.show(QuantityKind::RotationalSpeed, self.speed)
        );
        let fluid = format!(
            "{} · {}",
            self.fluid.trim(),
            units.show(QuantityKind::Temperature, self.inlet_temperature)
        );
        crate::steps::section(ui, tr("Summary"));
        props(ui, "new_project_summary", |ui| {
            value_row(ui, tr("Name"), self.name.trim().to_owned());
            value_row(ui, tr("Machine type"), tr("Centrifugal pump"));
            value_row(ui, tr("Design point"), point);
            value_row(ui, tr("Fluid"), fluid);
            value_row(ui, tr("Components"), chain.join(" → "));
        });
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
