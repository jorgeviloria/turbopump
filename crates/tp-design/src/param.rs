use serde::{Deserialize, Serialize};

/// A design parameter that is either proposed by the program or fixed by the user.
///
/// In `Auto` mode the value comes from a correlation evaluated at design time, so it
/// follows changes to upstream inputs. In `Manual` mode the user's value is kept.
///
/// Serialized as `{"mode":"auto"}` or `{"mode":"manual","value":...}`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", content = "value", rename_all = "snake_case")]
pub enum Param<T> {
    #[default]
    Auto,
    Manual(T),
}

impl<T: Copy> Param<T> {
    /// The effective value: the manual one, or `auto()` when in automatic mode.
    pub fn resolve(&self, auto: impl FnOnce() -> T) -> T {
        match *self {
            Param::Auto => auto(),
            Param::Manual(v) => v,
        }
    }

    pub fn manual_value(&self) -> Option<T> {
        match *self {
            Param::Auto => None,
            Param::Manual(v) => Some(v),
        }
    }

    pub fn is_auto(&self) -> bool {
        matches!(self, Param::Auto)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_auto_lazily() {
        let auto: Param<f64> = Param::Auto;
        assert_eq!(auto.resolve(|| 0.82), 0.82);
        let manual = Param::Manual(0.75);
        assert_eq!(manual.resolve(|| unreachable!()), 0.75);
        assert_eq!(manual.manual_value(), Some(0.75));
        assert!(Param::<u32>::default().is_auto());
    }

    #[test]
    fn serialized_form() {
        assert_eq!(
            serde_json::to_string(&Param::<f64>::Auto).unwrap(),
            r#"{"mode":"auto"}"#
        );
        assert_eq!(
            serde_json::to_string(&Param::Manual(7_u32)).unwrap(),
            r#"{"mode":"manual","value":7}"#
        );
        let back: Param<u32> = serde_json::from_str(r#"{"mode":"manual","value":7}"#).unwrap();
        assert_eq!(back, Param::Manual(7));
    }
}
