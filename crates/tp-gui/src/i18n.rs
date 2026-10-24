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

    #[test]
    fn language_codes_round_trip() {
        for l in Language::ALL {
            assert_eq!(Language::from_code(l.code()), Some(l));
        }
    }
}
