use crate::{QuantityKind, Unit, UnitError, find_unit};

/// Parses text such as `"2900 rpm"`, `"100m3/h"` or `"1,5 bar"` and returns the value in SI.
///
/// A comma is accepted as decimal separator when the number has no dot. The unit is
/// required, except for dimensionless quantities.
pub fn parse_quantity(text: &str, kind: QuantityKind) -> Result<f64, UnitError> {
    let (value, symbol) = split_number(text)?;
    if symbol.is_empty() {
        if kind == QuantityKind::Dimensionless {
            return Ok(value);
        }
        return Err(UnitError::MissingUnit {
            text: text.to_owned(),
            kind,
            example: kind.units().get(1).unwrap_or(kind.si_unit()).symbol,
        });
    }
    Ok(find_unit(kind, symbol)?.to_si(value))
}

/// Like [`parse_quantity`], but a bare number is read in `default_unit`.
pub fn parse_quantity_or(
    text: &str,
    kind: QuantityKind,
    default_unit: &Unit,
) -> Result<f64, UnitError> {
    let (value, symbol) = split_number(text)?;
    if symbol.is_empty() {
        Ok(default_unit.to_si(value))
    } else {
        Ok(find_unit(kind, symbol)?.to_si(value))
    }
}

/// Splits `text` into its leading number and the trimmed unit symbol after it.
fn split_number(text: &str) -> Result<(f64, &str), UnitError> {
    let invalid = || UnitError::InvalidNumber(text.to_owned());
    let t = text.trim();
    let numeric_end = t
        .find(|c: char| !(c.is_ascii_digit() || matches!(c, '+' | '-' | '.' | ',' | 'e' | 'E')))
        .unwrap_or(t.len());

    // The numeric prefix may swallow a trailing exponent marker that belongs to nothing,
    // so shorten it until it parses. All candidate characters are ASCII.
    for cut in (1..=numeric_end).rev() {
        let number = &t[..cut];
        if number.contains(',') && number.contains('.') {
            return Err(invalid());
        }
        if let Ok(value) = number.replace(',', ".").parse::<f64>() {
            if !value.is_finite() {
                return Err(invalid());
            }
            return Ok((value, t[cut..].trim()));
        }
    }
    Err(invalid())
}

#[cfg(test)]
mod tests {
    use super::*;
    use QuantityKind::*;
    use approx::assert_relative_eq;

    #[test]
    fn parses_with_and_without_space() {
        assert_relative_eq!(
            parse_quantity("100 m3/h", VolumeFlow).unwrap(),
            100.0 / 3600.0
        );
        assert_relative_eq!(
            parse_quantity("100m³/h", VolumeFlow).unwrap(),
            100.0 / 3600.0
        );
        assert_relative_eq!(
            parse_quantity("  2900rpm ", RotationalSpeed).unwrap(),
            2900.0 * std::f64::consts::PI / 30.0
        );
    }

    #[test]
    fn parses_signs_exponents_and_decimal_comma() {
        assert_relative_eq!(parse_quantity("-20 °C", Temperature).unwrap(), 253.15);
        assert_relative_eq!(parse_quantity("1.5e5 Pa", Pressure).unwrap(), 1.5e5);
        assert_relative_eq!(parse_quantity("1,5 bar", Pressure).unwrap(), 1.5e5);
        assert_relative_eq!(parse_quantity("2E-3 m", Length).unwrap(), 2e-3);
    }

    #[test]
    fn dimensionless_may_omit_unit() {
        assert_relative_eq!(parse_quantity("0.85", Dimensionless).unwrap(), 0.85);
        assert_relative_eq!(parse_quantity("85 %", Dimensionless).unwrap(), 0.85);
    }

    #[test]
    fn default_unit_applies_to_bare_numbers() {
        let mm = find_unit(Length, "mm").unwrap();
        assert_relative_eq!(parse_quantity_or("250", Length, mm).unwrap(), 0.25);
        assert_relative_eq!(parse_quantity_or("10 in", Length, mm).unwrap(), 0.254);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(matches!(
            parse_quantity("rpm", RotationalSpeed),
            Err(UnitError::InvalidNumber(_))
        ));
        assert!(matches!(
            parse_quantity("1,000.5 m", Length),
            Err(UnitError::InvalidNumber(_))
        ));
        assert!(matches!(
            parse_quantity("32", Head),
            Err(UnitError::MissingUnit { .. })
        ));
        assert!(matches!(
            parse_quantity("32 bar", Head),
            Err(UnitError::UnknownUnit { .. })
        ));
        assert!(matches!(
            parse_quantity("1e999 m", Length),
            Err(UnitError::InvalidNumber(_))
        ));
    }
}
