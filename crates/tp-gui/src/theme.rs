//! Look of the editor: fonts, icons, colors and spacing for the light and dark themes.
//!
//! Text uses Noto Sans, with egui's own fonts behind it for any symbol it lacks.
//! Icons come from Phosphor (MIT), mixed into ordinary text. Each kind of component
//! has its own color, used the same way in the component bar, the tree, the toolbar
//! and the 3D view.

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, RichText,
    Shadow, Stroke, TextStyle, Theme, Visuals,
};
use tp_project::ComponentKind;

pub(crate) use egui_phosphor::regular as icon;

/// Name of the bold font family.
const BOLD: &str = "bold";

/// Installs the fonts and the styles of both themes. Which theme is active stays the
/// caller's choice.
pub(crate) fn install(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    for theme in [Theme::Light, Theme::Dark] {
        ctx.style_mut_of(theme, |style| {
            style.visuals = visuals(theme);
            style.text_styles = [
                (TextStyle::Small, FontId::proportional(11.0)),
                (TextStyle::Body, FontId::proportional(13.0)),
                (TextStyle::Button, FontId::proportional(13.0)),
                (
                    TextStyle::Heading,
                    FontId::new(17.0, FontFamily::Name(BOLD.into())),
                ),
                (TextStyle::Monospace, FontId::monospace(12.0)),
            ]
            .into();
            let s = &mut style.spacing;
            s.item_spacing = egui::vec2(8.0, 5.0);
            s.button_padding = egui::vec2(8.0, 3.0);
            s.interact_size.y = 22.0;
            s.menu_margin = Margin::same(6);
            s.window_margin = Margin::same(12);
            s.indent = 16.0;
        });
    }
}

fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "noto-sans".into(),
        FontData::from_static(notosans::REGULAR_TTF).into(),
    );
    fonts.font_data.insert(
        "noto-sans-bold".into(),
        FontData::from_static(notosans::BOLD_TTF).into(),
    );
    let fallback = fonts.families[&FontFamily::Proportional].clone();
    let proportional = fonts
        .families
        .get_mut(&FontFamily::Proportional)
        .expect("egui defines proportional fonts");
    proportional.insert(0, "noto-sans".into());
    // Icons go right after the text font, ahead of egui's emoji fonts.
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    let mut bold = vec!["noto-sans-bold".to_owned(), "phosphor".to_owned()];
    bold.extend(fallback);
    fonts.families.insert(FontFamily::Name(BOLD.into()), bold);
    fonts
}

/// The bold font family.
pub(crate) fn bold_family() -> FontFamily {
    FontFamily::Name(BOLD.into())
}

/// `text` in the bold font.
pub(crate) fn bold(text: impl Into<String>) -> RichText {
    RichText::new(text).family(bold_family())
}

/// Colors of one theme beyond egui's visuals.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Palette {
    pub accent: Color32,
    /// Text on an accent-filled background.
    pub on_accent: Color32,
    /// Background of the toolbar body.
    pub ribbon: Color32,
    /// Background of the toolbar tab strip and the component bar.
    pub strip: Color32,
    pub border: Color32,
    pub hover: Color32,
    /// Selected rows and buttons.
    pub selected: Color32,
    pub text: Color32,
    pub weak: Color32,
    pub done: Color32,
    pub stale: Color32,
    pub warning_bg: Color32,
    pub error_bg: Color32,
}

impl Palette {
    pub fn of(ui: &egui::Ui) -> Self {
        Self::for_dark(ui.visuals().dark_mode)
    }

    pub fn for_dark(dark: bool) -> Self {
        if dark {
            Self {
                accent: Color32::from_rgb(91, 141, 239),
                on_accent: Color32::WHITE,
                ribbon: Color32::from_rgb(38, 42, 48),
                strip: Color32::from_rgb(27, 30, 35),
                border: Color32::from_rgb(56, 62, 70),
                hover: Color32::from_rgb(52, 58, 67),
                selected: Color32::from_rgb(44, 62, 96),
                text: Color32::from_rgb(226, 230, 235),
                weak: Color32::from_rgb(150, 158, 170),
                done: Color32::from_rgb(98, 196, 120),
                stale: Color32::from_rgb(232, 170, 70),
                warning_bg: Color32::from_rgb(62, 50, 28),
                error_bg: Color32::from_rgb(70, 34, 34),
            }
        } else {
            Self {
                accent: Color32::from_rgb(37, 99, 235),
                on_accent: Color32::WHITE,
                ribbon: Color32::WHITE,
                strip: Color32::from_rgb(233, 236, 241),
                border: Color32::from_rgb(212, 216, 223),
                hover: Color32::from_rgb(229, 234, 242),
                selected: Color32::from_rgb(214, 227, 251),
                text: Color32::from_rgb(29, 35, 43),
                weak: Color32::from_rgb(104, 113, 126),
                done: Color32::from_rgb(28, 135, 64),
                stale: Color32::from_rgb(190, 112, 0),
                warning_bg: Color32::from_rgb(253, 244, 222),
                error_bg: Color32::from_rgb(252, 230, 230),
            }
        }
    }
}

/// Identity color of a kind of component.
pub(crate) fn component_color(kind: ComponentKind) -> Color32 {
    match kind {
        ComponentKind::Impeller => Color32::from_rgb(196, 122, 44),
        ComponentKind::Stator { .. } => Color32::from_rgb(31, 158, 137),
        ComponentKind::Volute => Color32::from_rgb(47, 109, 181),
    }
}

fn visuals(theme: Theme) -> Visuals {
    let dark = theme == Theme::Dark;
    let p = Palette::for_dark(dark);
    let mut v = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    let radius = CornerRadius::same(4);
    let (panel, window, field, faint) = if dark {
        (
            Color32::from_rgb(31, 34, 39),
            Color32::from_rgb(38, 42, 48),
            Color32::from_rgb(22, 24, 28),
            Color32::from_rgb(36, 40, 46),
        )
    } else {
        (
            Color32::from_rgb(246, 247, 249),
            Color32::WHITE,
            Color32::WHITE,
            Color32::from_rgb(240, 242, 245),
        )
    };
    v.panel_fill = panel;
    v.window_fill = window;
    v.extreme_bg_color = field;
    v.text_edit_bg_color = Some(field);
    v.faint_bg_color = faint;
    v.code_bg_color = faint;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_corner_radius = CornerRadius::same(8);
    v.menu_corner_radius = CornerRadius::same(6);
    let shadow_alpha = if dark { 90 } else { 28 };
    v.window_shadow = Shadow {
        offset: [0, 6],
        blur: 18,
        spread: 0,
        color: Color32::from_black_alpha(shadow_alpha),
    };
    v.popup_shadow = Shadow {
        offset: [0, 4],
        blur: 12,
        spread: 0,
        color: Color32::from_black_alpha(shadow_alpha),
    };
    v.hyperlink_color = p.accent;
    v.selection.bg_fill = p.selected;
    v.selection.stroke = Stroke::new(1.0, p.accent);
    v.warn_fg_color = p.stale;
    v.error_fg_color = if dark {
        Color32::from_rgb(240, 110, 100)
    } else {
        Color32::from_rgb(196, 40, 40)
    };
    v.collapsing_header_frame = false;
    v.indent_has_left_vline = false;

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = panel;
    w.noninteractive.weak_bg_fill = panel;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.noninteractive.corner_radius = radius;

    let (button, button_hover, button_active) = if dark {
        (
            Color32::from_rgb(48, 53, 60),
            Color32::from_rgb(58, 64, 73),
            Color32::from_rgb(66, 73, 84),
        )
    } else {
        (
            Color32::from_rgb(236, 239, 243),
            Color32::from_rgb(225, 231, 240),
            Color32::from_rgb(210, 220, 236),
        )
    };
    w.inactive.bg_fill = button;
    w.inactive.weak_bg_fill = button;
    w.inactive.bg_stroke = Stroke::new(1.0, p.border);
    w.inactive.fg_stroke = Stroke::new(1.0, p.text);
    w.inactive.corner_radius = radius;
    // egui lays a button out without its frame as padding + expansion − stroke of the
    // state, and with its frame as the padding alone: an expansion equal to the stroke
    // keeps buttons whose frame shows only on hover or selection from growing then, and
    // the same expansion in every state keeps their frames from jumping.
    w.inactive.expansion = 1.0;

    w.hovered.bg_fill = button_hover;
    w.hovered.weak_bg_fill = button_hover;
    w.hovered.bg_stroke = Stroke::new(1.0, p.accent.gamma_multiply(0.6));
    w.hovered.fg_stroke = Stroke::new(1.5, p.text);
    w.hovered.corner_radius = radius;
    w.hovered.expansion = 1.0;

    w.active.bg_fill = button_active;
    w.active.weak_bg_fill = button_active;
    w.active.bg_stroke = Stroke::new(1.0, p.accent);
    w.active.fg_stroke = Stroke::new(1.5, p.text);
    w.active.corner_radius = radius;
    w.active.expansion = 1.0;

    w.open.bg_fill = button_hover;
    w.open.weak_bg_fill = button_hover;
    w.open.bg_stroke = Stroke::new(1.0, p.accent.gamma_multiply(0.6));
    w.open.fg_stroke = Stroke::new(1.0, p.text);
    w.open.corner_radius = radius;
    w.open.expansion = 1.0;
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every non-ASCII symbol the editor writes must exist in its fonts, or it is drawn
    /// as an empty box.
    #[test]
    fn symbols_used_by_the_editor_have_glyphs() {
        let symbols = format!(
            "°±²³·éαβγδεηθνξρτψϑ–—•…₁₂₃₄−≤≥{}",
            [
                icon::CHECK_CIRCLE,
                icon::WARNING,
                icon::X_CIRCLE,
                icon::GEAR,
                icon::CUBE,
                icon::FLOPPY_DISK,
            ]
            .concat()
        );
        let ctx = egui::Context::default();
        install(&ctx);
        let mut output = ctx.run_ui(egui::RawInput::default(), |_| {});
        output.textures_delta.clear();
        for family in [FontFamily::Proportional, FontFamily::Name(BOLD.into())] {
            let missing: String = ctx.fonts_mut(|fonts| {
                symbols
                    .chars()
                    .filter(|&c| !fonts.has_glyph(&FontId::new(13.0, family.clone()), c))
                    .collect()
            });
            assert!(missing.is_empty(), "{family:?}: no glyph for {missing:?}");
        }
    }

    #[test]
    fn components_have_distinct_colors() {
        let colors = [
            component_color(ComponentKind::Impeller),
            component_color(ComponentKind::Stator { vaned: true }),
            component_color(ComponentKind::Volute),
        ];
        assert_ne!(colors[0], colors[1]);
        assert_ne!(colors[1], colors[2]);
        assert_eq!(
            component_color(ComponentKind::Stator { vaned: false }),
            colors[1]
        );
    }
}
