//! CoolProp through its C API, loaded at run time.
//!
//! I. H. Bell, J. Wronski, S. Quoilin, V. Lemort, "Pure and pseudo-pure fluid
//! thermophysical property evaluation and the open-source thermophysical property
//! library CoolProp", *Ind. Eng. Chem. Res.* 53 (2014) 2498–2508.
//!
//! The shared library is looked up, in order, at:
//! 1. the file named by the `TURBOPUMP_COOLPROP` environment variable;
//! 2. next to the executable, then in `../lib` and `../Frameworks` relative to it;
//! 3. the system library search path.
//!
//! `TURBOPUMP_COOLPROP=none` disables CoolProp, leaving the built-in water model.
//!
//! Loading at run time (instead of linking) lets the program start without CoolProp
//! and lets installers ship the library next to the executable.

// FFI with a C library is inherently unsafe. This is the only module of the workspace
// allowed to use `unsafe`; every block states why it is sound.
#![allow(unsafe_code)]

use std::ffi::{CStr, CString, c_char, c_double, c_int, c_long};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use libloading::Library;

use crate::{FluidError, LiquidProperties, PropertySource};

/// Environment variable with the full path of the CoolProp shared library.
pub const ENV_VAR: &str = "TURBOPUMP_COOLPROP";

const LIBRARY_FILE: &str = if cfg!(windows) {
    "CoolProp.dll"
} else if cfg!(target_os = "macos") {
    "libCoolProp.dylib"
} else {
    "libCoolProp.so"
};

type PropsSiFn = unsafe extern "C" fn(
    *const c_char,
    *const c_char,
    c_double,
    *const c_char,
    c_double,
    *const c_char,
) -> c_double;
type Props1SiFn = unsafe extern "C" fn(*const c_char, *const c_char) -> c_double;
type GlobalParamFn = unsafe extern "C" fn(*const c_char, *mut c_char, c_int) -> c_long;

pub(crate) struct CoolProp {
    props_si: PropsSiFn,
    props1_si: Props1SiFn,
    global_param: GlobalParamFn,
    /// CoolProp keeps global state (the last error); calls are serialized.
    lock: Mutex<()>,
    path: PathBuf,
    /// Keeps the function pointers valid. Never dropped: it lives in a static.
    _library: Library,
}

static COOLPROP: OnceLock<Result<CoolProp, String>> = OnceLock::new();

fn open(path: &Path) -> Result<CoolProp, String> {
    // SAFETY: loading a library runs its initializers. We only load files named
    // CoolProp from the locations documented above or chosen by the user.
    let library = unsafe { Library::new(path) }.map_err(|e| format!("{}: {e}", path.display()))?;
    // SAFETY: the symbol types match the declarations in CoolPropLib.h. The copied
    // function pointers stay valid because `library` is stored alongside them.
    let (props_si, props1_si, global_param) = unsafe {
        let props_si: PropsSiFn = *library
            .get::<PropsSiFn>(b"PropsSI\0")
            .map_err(|e| e.to_string())?;
        let props1_si: Props1SiFn = *library
            .get::<Props1SiFn>(b"Props1SI\0")
            .map_err(|e| e.to_string())?;
        let global_param: GlobalParamFn = *library
            .get::<GlobalParamFn>(b"get_global_param_string\0")
            .map_err(|e| e.to_string())?;
        (props_si, props1_si, global_param)
    };
    let coolprop = CoolProp {
        props_si,
        props1_si,
        global_param,
        lock: Mutex::new(()),
        path: path.to_owned(),
        _library: library,
    };
    coolprop
        .props1("Water", "Tcrit")
        .map_err(|e| format!("{}: probe failed: {e}", path.display()))?;
    Ok(coolprop)
}

fn candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(path) = std::env::var_os(ENV_VAR) {
        paths.push(PathBuf::from(path));
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_owned))
    {
        paths.push(dir.join(LIBRARY_FILE));
        paths.push(dir.join("../lib").join(LIBRARY_FILE));
        paths.push(dir.join("../Frameworks").join(LIBRARY_FILE));
    }
    paths.push(PathBuf::from(LIBRARY_FILE));
    paths
}

/// Value of [`ENV_VAR`] that disables CoolProp.
pub const DISABLED: &str = "none";

fn search() -> Result<CoolProp, String> {
    if std::env::var_os(ENV_VAR).is_some_and(|v| v == DISABLED) {
        return Err(format!("CoolProp disabled by {ENV_VAR}={DISABLED}"));
    }
    let mut tried = Vec::new();
    for path in candidates() {
        match open(&path) {
            Ok(lib) => return Ok(lib),
            Err(e) => tried.push(e),
        }
    }
    Err(format!(
        "CoolProp shared library not found; place {LIBRARY_FILE} next to the executable or \
         set {ENV_VAR} to its path (tried: {})",
        tried.join("; ")
    ))
}

/// The loaded library, searching for it on first use.
pub(crate) fn library() -> Result<&'static CoolProp, FluidError> {
    COOLPROP
        .get_or_init(search)
        .as_ref()
        .map_err(|e| FluidError::CoolPropUnavailable(e.clone()))
}

/// Loads CoolProp from `path` instead of searching for it. Must be called before any
/// property evaluation; afterwards the library in use cannot change.
pub fn use_coolprop_library(path: impl AsRef<Path>) -> Result<(), FluidError> {
    let path = path.as_ref();
    let loaded = COOLPROP.get_or_init(|| open(path));
    match loaded {
        Ok(lib) if lib.path == path => Ok(()),
        Ok(lib) => Err(FluidError::CoolPropUnavailable(format!(
            "CoolProp is already loaded from {}",
            lib.path.display()
        ))),
        Err(e) => Err(FluidError::CoolPropUnavailable(e.clone())),
    }
}

/// Path of the CoolProp library in use, if it could be loaded.
pub fn coolprop_library_path() -> Option<&'static Path> {
    library().ok().map(|lib| lib.path.as_path())
}

fn c_string(s: &str) -> Result<CString, String> {
    CString::new(s).map_err(|_| format!("invalid name {s:?}"))
}

impl CoolProp {
    /// Reads CoolProp's last error message. Call with `lock` held.
    fn last_error(&self) -> String {
        let mut buffer = vec![0 as c_char; 2048];
        // SAFETY: the buffer is writable for its stated length and CoolProp writes a
        // NUL-terminated string of at most that length.
        unsafe {
            (self.global_param)(
                c"errstring".as_ptr(),
                buffer.as_mut_ptr(),
                buffer.len() as c_int,
            );
            CStr::from_ptr(buffer.as_ptr())
        }
        .to_string_lossy()
        .into_owned()
    }

    fn check(&self, value: f64) -> Result<f64, String> {
        if value.is_finite() && value.abs() < 1e300 {
            Ok(value)
        } else {
            Err(self.last_error())
        }
    }

    /// `PropsSI(output, name1, value1, name2, value2, fluid)`.
    fn props(
        &self,
        output: &str,
        name1: &str,
        value1: f64,
        name2: &str,
        value2: f64,
        fluid: &str,
    ) -> Result<f64, String> {
        let (o, n1, n2, f) = (
            c_string(output)?,
            c_string(name1)?,
            c_string(name2)?,
            c_string(fluid)?,
        );
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        // SAFETY: all pointers are valid NUL-terminated strings that outlive the call;
        // CoolProp does not retain them.
        let value = unsafe {
            (self.props_si)(
                o.as_ptr(),
                n1.as_ptr(),
                value1,
                n2.as_ptr(),
                value2,
                f.as_ptr(),
            )
        };
        self.check(value)
    }

    /// `Props1SI(fluid, output)`: properties that need no state, such as `Tcrit`.
    fn props1(&self, fluid: &str, output: &str) -> Result<f64, String> {
        let (f, o) = (c_string(fluid)?, c_string(output)?);
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        // SAFETY: as in `props`.
        let value = unsafe { (self.props1_si)(f.as_ptr(), o.as_ptr()) };
        self.check(value)
    }
}

pub(crate) fn liquid_properties(
    lib: &CoolProp,
    name: &str,
    pressure: f64,
    temperature: f64,
) -> Result<LiquidProperties, FluidError> {
    let fluid = name.trim();
    let critical_temperature = lib
        .props1(fluid, "Tcrit")
        .map_err(|_| FluidError::UnknownFluid(name.to_owned()))?;
    let not_liquid = |vapor_pressure| FluidError::NotLiquid {
        fluid: name.to_owned(),
        pressure,
        temperature,
        vapor_pressure,
    };
    if temperature >= critical_temperature {
        return Err(not_liquid(f64::NAN));
    }
    let vapor_pressure = lib
        .props("P", "T", temperature, "Q", 0.0, fluid)
        .map_err(FluidError::CoolProp)?;
    if pressure <= vapor_pressure {
        return Err(not_liquid(vapor_pressure));
    }
    let state = |output| {
        lib.props(output, "P", pressure, "T", temperature, fluid)
            .map_err(FluidError::CoolProp)
    };
    Ok(LiquidProperties {
        density: state("D")?,
        dynamic_viscosity: state("V")?,
        vapor_pressure,
        source: PropertySource::CoolProp,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::water;
    use approx::assert_relative_eq;

    /// Loads the CoolProp build supplied by the `coolprop-sys` dev-dependency.
    pub(crate) fn coolprop() -> &'static CoolProp {
        use_coolprop_library(coolprop_sys::COOLPROP_PATH).unwrap();
        library().unwrap()
    }

    #[test]
    fn water_agrees_with_builtin_model() {
        let lib = coolprop();
        for celsius in [5.0, 20.0, 50.0, 80.0] {
            let t = 273.15 + celsius;
            let cp = liquid_properties(lib, "Water", 101_325.0, t).unwrap();
            let builtin = water::liquid_properties(101_325.0, t).unwrap();
            assert_eq!(cp.source, PropertySource::CoolProp);
            assert_relative_eq!(cp.density, builtin.density, max_relative = 5e-4);
            assert_relative_eq!(
                cp.vapor_pressure,
                builtin.vapor_pressure,
                max_relative = 1e-4
            );
            assert_relative_eq!(
                cp.dynamic_viscosity,
                builtin.dynamic_viscosity,
                max_relative = 0.012
            );
        }
    }

    #[test]
    fn other_liquids_by_name() {
        let lib = coolprop();
        let ethanol = liquid_properties(lib, "Ethanol", 101_325.0, 293.15).unwrap();
        assert_relative_eq!(ethanol.density, 789.0, max_relative = 0.005);
        assert!(liquid_properties(lib, "H2O", 101_325.0, 293.15).is_ok());
    }

    #[test]
    fn rejects_unknown_and_non_liquid_states() {
        let lib = coolprop();
        assert!(matches!(
            liquid_properties(lib, "Unobtainium", 101_325.0, 293.15),
            Err(FluidError::UnknownFluid(_))
        ));
        assert!(matches!(
            liquid_properties(lib, "Water", 101_325.0, 393.15),
            Err(FluidError::NotLiquid { .. })
        ));
        assert!(matches!(
            liquid_properties(lib, "Water", 30e6, 700.0),
            Err(FluidError::NotLiquid { .. })
        ));
        // The last error is reported, not swallowed.
        let err = lib
            .props("D", "P", 1e5, "T", 300.0, "Unobtainium")
            .unwrap_err();
        assert!(!err.is_empty());
    }

    #[test]
    fn library_path_is_reported() {
        coolprop();
        assert_eq!(
            coolprop_library_path(),
            Some(Path::new(coolprop_sys::COOLPROP_PATH))
        );
    }
}
