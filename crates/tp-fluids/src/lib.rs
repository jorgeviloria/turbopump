//! Working-fluid properties.
//!
//! Pumps need three liquid properties at the inlet state: density, viscosity and vapour
//! pressure. They come from CoolProp when its shared library can be loaded, and from a
//! built-in IAPWS-based water model otherwise, so the program works for water even
//! without CoolProp.

#[cfg(feature = "coolprop")]
mod coolprop;
mod water;

#[cfg(feature = "coolprop")]
pub use coolprop::{ENV_VAR as COOLPROP_ENV_VAR, coolprop_library_path, use_coolprop_library};
pub use water::{
    BUILTIN_WATER_RANGE, saturated_liquid_density, saturation_pressure, water_viscosity,
};

/// Where a set of properties came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertySource {
    /// Built-in correlations for water (see [`saturation_pressure`]).
    BuiltinWater,
    /// CoolProp (reference equations of state).
    CoolProp,
}

/// Properties of a liquid at a given pressure and temperature, in SI units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LiquidProperties {
    /// kg/m³
    pub density: f64,
    /// Pa·s
    pub dynamic_viscosity: f64,
    /// Saturation pressure at the given temperature, Pa.
    pub vapor_pressure: f64,
    pub source: PropertySource,
}

impl LiquidProperties {
    /// m²/s
    pub fn kinematic_viscosity(&self) -> f64 {
        self.dynamic_viscosity / self.density
    }
}

/// Errors from property evaluation.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum FluidError {
    #[error("unknown fluid {0:?}")]
    UnknownFluid(String),

    #[error(
        "{fluid} is not liquid at {pressure} Pa and {temperature} K \
         (vapour pressure {vapor_pressure} Pa)"
    )]
    NotLiquid {
        fluid: String,
        pressure: f64,
        temperature: f64,
        vapor_pressure: f64,
    },

    #[error("{model} is valid between {min} K and {max} K, got {temperature} K")]
    OutOfRange {
        model: &'static str,
        temperature: f64,
        min: f64,
        max: f64,
    },

    #[error("{0}")]
    CoolPropUnavailable(String),

    #[error("CoolProp: {0}")]
    CoolProp(String),
}

/// Liquid properties of the fluid called `name` at `pressure` (Pa) and `temperature` (K).
///
/// Uses CoolProp when its library is available, which knows about 120 pure fluids by
/// name ("Water", "Ethanol", "n-Hexane", ...). Without it only water is available,
/// from the built-in model.
pub fn liquid_properties(
    name: &str,
    pressure: f64,
    temperature: f64,
) -> Result<LiquidProperties, FluidError> {
    #[cfg(feature = "coolprop")]
    let unavailable = match coolprop::library() {
        Ok(lib) => return coolprop::liquid_properties(lib, name, pressure, temperature),
        Err(e) => e,
    };
    #[cfg(not(feature = "coolprop"))]
    let unavailable = FluidError::CoolPropUnavailable(
        "this build has no CoolProp support; only water is available".into(),
    );

    if is_water(name) {
        water::liquid_properties(pressure, temperature)
    } else {
        Err(unavailable)
    }
}

/// Liquid water from the built-in model, regardless of whether CoolProp is available.
pub fn builtin_water(pressure: f64, temperature: f64) -> Result<LiquidProperties, FluidError> {
    water::liquid_properties(pressure, temperature)
}

/// Whether the CoolProp library could be loaded.
pub fn coolprop_available() -> bool {
    #[cfg(feature = "coolprop")]
    {
        coolprop::library().is_ok()
    }
    #[cfg(not(feature = "coolprop"))]
    {
        false
    }
}

fn is_water(name: &str) -> bool {
    matches!(name.trim().to_ascii_lowercase().as_str(), "water" | "h2o")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn water_is_always_available() {
        let props = liquid_properties("Water", 101_325.0, 293.15).unwrap();
        assert!((998.0..998.5).contains(&props.density));
        assert!(is_water(" H2O "));
    }

    #[cfg(feature = "coolprop")]
    #[test]
    fn dispatches_to_coolprop_once_loaded() {
        coolprop::tests::coolprop();
        assert!(coolprop_available());
        let props = liquid_properties("Ethanol", 101_325.0, 293.15).unwrap();
        assert_eq!(props.source, PropertySource::CoolProp);
    }
}
