//! Interface language. The English texts in the source are the keys; the Spanish
//! translations live in `es.rs`.
//!
//! - [`tr`] translates a fixed text.
//! - [`tf`] translates a template whose `{}` are filled in order.
//! - [`message`] translates a message built by the design code (a warning or an error)
//!   by matching it against templates, keeping the numbers and names inside it.

use std::cell::Cell;
use std::collections::HashMap;
use std::fmt::Display;
use std::sync::LazyLock;

mod es;

/// The languages of the interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Language {
    #[default]
    English,
    Spanish,
}

impl Language {
    pub const ALL: [Language; 2] = [Language::English, Language::Spanish];

    /// The language's own name.
    pub fn name(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::Spanish => "Español",
        }
    }

    /// Code stored in the preferences.
    pub fn code(self) -> &'static str {
        match self {
            Language::English => "en",
            Language::Spanish => "es",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.code() == code)
    }
}

thread_local! {
    // The interface runs on one thread; tests each get their own.
    static CURRENT: Cell<Language> = const { Cell::new(Language::English) };
}

/// Sets the language of the interface on this thread.
pub fn set(language: Language) {
    CURRENT.with(|c| c.set(language));
}

/// The language of the interface on this thread.
pub fn current() -> Language {
    CURRENT.with(Cell::get)
}

static SPANISH: LazyLock<HashMap<&'static str, &'static str>> =
    LazyLock::new(|| es::TEXTS.iter().copied().collect());

static ENGLISH: LazyLock<HashMap<&'static str, &'static str>> =
    LazyLock::new(|| es::TEXTS.iter().map(|&(en, es)| (es, en)).collect());

/// The English text that `text`, shown in the current language, translates: the key
/// that names it whatever the language.
pub fn key_of(text: &str) -> &str {
    match current() {
        Language::English => text,
        Language::Spanish => ENGLISH.get(text).copied().unwrap_or(text),
    }
}

/// `text` in the current language.
pub fn tr(text: &'static str) -> &'static str {
    match current() {
        Language::English => text,
        Language::Spanish => SPANISH.get(text).copied().unwrap_or(text),
    }
}

/// Fills the `{}` of `template` with `args`, in order. Placeholders written `{0}`,
/// `{1}`… take the argument of that index, so a translation can reorder them.
fn fill(template: &str, args: &[String]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut next = 0;
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('}') {
            Some(end) if after[..end].chars().all(|c| c.is_ascii_digit()) => {
                let index = if end == 0 {
                    next += 1;
                    next - 1
                } else {
                    after[..end].parse().unwrap_or(0)
                };
                out.push_str(args.get(index).map_or("", String::as_str));
                rest = &after[end + 1..];
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// `template` in the current language, with its `{}` filled by `args` in order.
pub fn tf(template: &'static str, args: &[&dyn Display]) -> String {
    let args: Vec<String> = args.iter().map(ToString::to_string).collect();
    fill(tr(template), &args)
}

/// The name of a fluid to show: the common CoolProp names in words of the current
/// language ("HeavyWater" is heavy water); any other name as typed.
pub fn fluid_name(name: &str) -> String {
    let words = match name {
        "Water" => "Water",
        "HeavyWater" => "Heavy water",
        "Ethanol" => "Ethanol",
        "Methanol" => "Methanol",
        "Acetone" => "Acetone",
        "Toluene" => "Toluene",
        "n-Hexane" => "n-Hexane",
        "n-Heptane" => "n-Heptane",
        "n-Decane" => "n-Decane",
        "Ammonia" => "Ammonia",
        "Propane" => "Propane",
        other => return other.to_owned(),
    };
    tr(words).to_owned()
}

/// A component name in the current language: the default names ("Impeller 2") are
/// translated; names the user typed stay as they are.
pub fn component_name(name: &str) -> String {
    const DEFAULTS: [&str; 4] = ["Vaneless stator", "Impeller", "Stator", "Volute"];
    for prefix in DEFAULTS {
        if let Some(number) = name.strip_prefix(prefix).and_then(|r| r.strip_prefix(' '))
            && !number.is_empty()
            && number.chars().all(|c| c.is_ascii_digit())
        {
            return format!("{} {number}", tr(prefix));
        }
    }
    name.to_owned()
}

/// A project name in the current language: the default name is translated.
pub fn project_name(name: &str) -> &str {
    if name == "Untitled" {
        tr("Untitled")
    } else {
        name
    }
}

/// The variable parts of `text` if it follows `template`, whose `{}` stand for them.
fn captures(template: &str, text: &str) -> Option<Vec<String>> {
    let parts: Vec<&str> = template.split("{}").collect();
    let mut rest = text.strip_prefix(parts[0])?;
    let mut values = Vec::with_capacity(parts.len() - 1);
    for (i, part) in parts.iter().enumerate().skip(1) {
        let last = i == parts.len() - 1;
        let at = if last {
            if !rest.ends_with(part) {
                return None;
            }
            rest.len() - part.len()
        } else if part.is_empty() {
            return None;
        } else {
            rest.find(part)?
        };
        values.push(rest[..at].to_owned());
        rest = &rest[at + part.len()..];
    }
    rest.is_empty().then_some(values)
}

/// A message from the design code in the current language. Messages without a
/// translation stay as they are.
pub fn message(text: &str) -> String {
    if current() == Language::English {
        return text.to_owned();
    }
    for (english, spanish) in es::MESSAGES {
        if let Some(values) = captures(english, text) {
            let values: Vec<String> = values.iter().map(|v| message(v)).collect();
            return fill(spanish, &values);
        }
    }
    // A message may be a known text, or a known text after a "label: " prefix.
    if let Some(&translated) = SPANISH.get(text) {
        return translated.to_owned();
    }
    if let Some((head, tail)) = text.split_once(": ") {
        let head_es = SPANISH.get(head).copied().unwrap_or(head);
        return format!("{head_es}: {}", message(tail));
    }
    text.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spanish<T>(f: impl FnOnce() -> T) -> T {
        set(Language::Spanish);
        let out = f();
        set(Language::English);
        out
    }

    #[test]
    fn crossing_walls_are_named_in_spanish() {
        let text = spanish(|| message("walls cross: hub disk and hub at z = 7.1 mm, r = 9.6 mm"));
        assert_eq!(
            text,
            "paredes que se cruzan: disco del cubo y cubo, en z = 7.1 mm, r = 9.6 mm"
        );
    }

    #[test]
    fn english_is_the_identity() {
        set(Language::English);
        assert_eq!(tr("Accept step"), "Accept step");
        assert_eq!(
            tf("{} accepted", &[&"Main dimensions"]),
            "Main dimensions accepted"
        );
        assert_eq!(message("anything at all"), "anything at all");
    }

    #[test]
    fn templates_fill_in_order_or_by_index() {
        let args = ["a".to_owned(), "b".to_owned()];
        assert_eq!(fill("{} then {}", &args), "a then b");
        assert_eq!(fill("{1} before {0}", &args), "b before a");
        assert_eq!(fill("braces {x} stay", &args), "braces {x} stay");
    }

    #[test]
    fn captures_find_the_variable_parts() {
        assert_eq!(
            captures(
                "the head reaches zero at {} % of the design flow",
                "the head reaches zero at 140 % of the design flow"
            ),
            Some(vec!["140".to_owned()])
        );
        assert_eq!(
            captures("{} vanes with {} blades", "12 vanes with 7 blades"),
            Some(vec!["12".into(), "7".into()])
        );
        assert_eq!(captures("exact text", "other text"), None);
    }

    #[test]
    fn spanish_translates_texts_templates_and_messages() {
        spanish(|| {
            assert_eq!(tr("Accept step"), "Aceptar paso");
            assert_eq!(tf("{} accepted", &[&"X"]), "Paso aceptado: X");
            assert_eq!(project_name("Untitled"), "Sin título");
            assert_eq!(project_name("Bomba 3"), "Bomba 3");
            let m = message("the predicted best efficiency is at 90 % of the design flow");
            assert!(m.starts_with("el máximo rendimiento previsto"), "{m}");
            assert!(m.contains("90"));
            let m = message(
                "NPSH₃ exceeds the NPSH available of 10.11 m above 143 % of the design flow",
            );
            assert_eq!(
                m,
                "el NPSH₃ supera el NPSH disponible de 10.11 m por encima del 143 % del caudal de diseño"
            );
            let m = message("the NPSH₃ exponent must lie between 1 and 6, got 0.2");
            assert_eq!(
                m,
                "el exponente del NPSH₃ debe estar entre 1 y 6; se recibió 0.2"
            );
        });
    }

    #[test]
    fn default_component_names_are_translated() {
        spanish(|| {
            assert_eq!(component_name("Impeller 2"), "Rodete 2");
            assert_eq!(component_name("Vaneless stator 1"), "Estator sin álabes 1");
            assert_eq!(component_name("Impeller of stage 2"), "Impeller of stage 2");
        });
        assert_eq!(component_name("Volute 1"), "Volute 1");
    }

    /// The runtime value of a string literal's source text: escapes and line
    /// continuations resolved.
    fn literal_value(raw: &str) -> String {
        let mut out = String::new();
        let mut chars = raw.chars().peekable();
        while let Some(c) = chars.next() {
            if c != '\\' {
                out.push(c);
                continue;
            }
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('\n') => {
                    while chars.peek().is_some_and(|c| c.is_whitespace()) {
                        chars.next();
                    }
                }
                Some(other) => out.push(other),
                None => {}
            }
        }
        out
    }

    /// Every `tr("…")` and `tf("…")` in the interface has a Spanish translation.
    #[test]
    fn every_translated_text_has_spanish() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = vec![root];
        let mut missing = Vec::new();
        while let Some(path) = files.pop() {
            if path.is_dir() {
                if path.ends_with("i18n") {
                    continue;
                }
                for entry in std::fs::read_dir(&path).unwrap() {
                    files.push(entry.unwrap().path());
                }
                continue;
            }
            if path.extension().is_none_or(|e| e != "rs") || path.ends_with("i18n.rs") {
                continue;
            }
            // A Windows checkout may have CRLF line endings; rustc reads them as LF.
            let source = std::fs::read_to_string(&path)
                .unwrap()
                .replace("\r\n", "\n");
            let source = source.split("#[cfg(test)]").next().unwrap_or("");
            for call in ["tr(", "tf("] {
                for (at, _) in source.match_indices(call) {
                    // Only the function itself, not names that end in it.
                    let before = source[..at].chars().last();
                    if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                        continue;
                    }
                    let rest = source[at + call.len()..].trim_start();
                    let Some(rest) = rest.strip_prefix('"') else {
                        continue;
                    };
                    let mut end = 0;
                    let bytes = rest.as_bytes();
                    while end < bytes.len() && bytes[end] != b'"' {
                        end += if bytes[end] == b'\\' { 2 } else { 1 };
                    }
                    let text = literal_value(&rest[..end]);
                    // Symbols such as "c₃q/c₂" read the same in every language.
                    let has_word = text
                        .split(|c: char| !c.is_ascii_alphabetic())
                        .any(|w| w.len() >= 3);
                    if has_word && !SPANISH.contains_key(text.as_str()) {
                        missing.push(format!("{}: {text:?}", path.display()));
                    }
                }
            }
        }
        assert!(
            missing.is_empty(),
            "without Spanish:\n{}",
            missing.join("\n")
        );
    }

    #[test]
    fn translations_keep_their_placeholders() {
        let count = |s: &str| s.matches("{}").count();
        for (en, es) in es::TEXTS.iter().chain(es::MESSAGES) {
            let indexed = (0..9).filter(|i| es.contains(&format!("{{{i}}}"))).count();
            assert_eq!(count(en), count(es) + indexed, "{en:?} → {es:?}");
        }
    }

    /// The warnings the seals, the back vanes and the chambers raise on odd inputs have
    /// a Spanish message (the check above only sees the literals of `tr` and `tf`).
    #[test]
    fn contour_warnings_have_spanish() {
        use tp_design::Param::Manual;
        use tp_design::impeller::back_vanes::VaneShape;
        let project = crate::default_project();
        let id = project.components()[0].id();
        type Change = Box<dyn Fn(&mut tp_design::impeller::ImpellerGeometryInput)>;
        let cases: Vec<Change> = vec![
            Box::new(|i| i.meridional.seals.front.diameter = Manual(0.001)),
            Box::new(|i| {
                i.meridional.seals.front.impeller_ring.enabled = false;
                i.meridional.seals.front.diameter = Manual(0.001);
            }),
            Box::new(|i| i.meridional.seals.front.impeller_ring.thickness = Manual(0.5)),
            Box::new(|i| i.meridional.seals.front.clearance = Manual(1e-6)),
            Box::new(|i| i.meridional.seals.rear.diameter = Manual(0.5)),
            Box::new(|i| i.meridional.seals.front.enabled = false),
            Box::new(|i| {
                i.meridional.chambers.enabled = true;
                i.meridional.seals.front.enabled = false;
                i.meridional.back_vanes.enabled = true;
                i.meridional.back_vanes.outer_diameter = Manual(1.0);
                i.meridional.back_vanes.inner_diameter = Manual(0.02);
                i.meridional.back_vanes.height = Manual(0.5);
                i.meridional.back_vanes.shape = VaneShape::Curved { angle: 0.5 };
            }),
            Box::new(|i| {
                i.meridional.back_vanes.enabled = true;
                i.meridional.back_vanes.outer_diameter = Manual(0.02);
            }),
            Box::new(|i| {
                i.meridional.chambers.enabled = true;
                i.meridional.back_vanes.enabled = true;
                i.meridional.seals.rear.diameter = Manual(0.12);
            }),
        ];
        let mut warnings = Vec::new();
        for case in &cases {
            let mut p = project.clone();
            let mut input = p.impeller_geometry_input(id).unwrap();
            case(&mut input);
            p.set_impeller_geometry(id, input).unwrap();
            if let Ok(g) = p.compute_impeller_geometry(id) {
                warnings.extend(g.warnings);
            }
        }
        for needle in [
            "impeller ring of",
            "below the neck wall",
            "Gülich recommends",
            "reaches beyond the hub disk rim",
            "front seal is off",
            "reach out to",
            "no room",
        ] {
            assert!(
                warnings.iter().any(|w| w.contains(needle)),
                "no warning with {needle:?}: {warnings:#?}"
            );
        }
        let untranslated: Vec<_> = warnings
            .iter()
            .filter(|w| !w.contains("cross") && spanish(|| message(w)) == **w)
            .collect();
        assert!(
            untranslated.is_empty(),
            "without Spanish: {untranslated:#?}"
        );
    }

    #[test]
    fn ring_and_seal_names_inside_messages_are_translated() {
        let text = spanish(|| {
            message(
                "the front impeller ring of 6.0 mm is trimmed to 2.0 mm to leave the neck a wall",
            )
        });
        assert_eq!(
            text,
            "el anillo delantero del rodete, de 6.0 mm, se recorta a 2.0 mm para dejar una pared al cuello"
        );
        let text =
            spanish(|| message("the rear casing wear ring is left out: it has no radial extent"));
        assert_eq!(
            text,
            "el anillo de desgaste trasero de la carcasa se omite: no tiene extensión radial"
        );
    }

    #[test]
    fn the_side_of_a_seal_follows_the_seal_and_specific_messages_win() {
        let text = spanish(|| {
            message(
                "the front seal clearance 0.1 mm is below the 0.3 mm Gülich recommends for a seal of 92.1 mm",
            )
        });
        assert_eq!(
            text,
            "la holgura del sello delantero, de 0.1 mm, es menor que los 0.3 mm que recomienda Gülich para un sello de 92.1 mm"
        );
        let text = spanish(|| {
            message(
                "the rear seal diameter 80.0 mm is below the neck wall (85.0 mm): the seal cuts into the neck",
            )
        });
        assert_eq!(
            text,
            "el diámetro del sello trasero, de 80.0 mm, queda por debajo de la pared del cuello (85.0 mm): el sello corta el cuello"
        );
        let text =
            spanish(|| message("the back vanes could not be taken out of the side chambers"));
        assert_eq!(
            text,
            "no se pudieron restar los álabes de rebombeo de las cámaras laterales"
        );
    }

    #[test]
    fn language_codes_round_trip() {
        for l in Language::ALL {
            assert_eq!(Language::from_code(l.code()), Some(l));
        }
    }
}
