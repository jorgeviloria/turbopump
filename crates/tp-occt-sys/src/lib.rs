//! Builds Open CASCADE Technology (OCCT) 8.0.1 from the sources in `OCCT/` (a git
//! submodule) as static libraries, for use from a build script.
//!
//! The published `occt-sys` crate stops at OCCT 7.8.1, and its patching of the module
//! list relies on the `BUILD_PATCH` option that OCCT 7.9.1 removed. Here the modules are
//! chosen with OCCT's own `BUILD_MODULE_*` options instead: modelling, data exchange
//! and the application framework that XCAF needs, without Draw, the viewers or any
//! third-party library.

use std::path::{Path, PathBuf};

/// The OCCT version these sources hold.
pub const OCCT_VERSION: &str = "8.0.1";

const LIB_DIR: &str = "lib";
const INCLUDE_DIR: &str = "include";

/// Where OCCT is installed: `target/OCCT-<version>`, outside the build directory of
/// any one crate, so a change to a crate does not rebuild it and `cargo clean` does.
///
/// Only valid while a build script runs.
pub fn occt_path() -> PathBuf {
    let out = std::env::var("OUT_DIR").expect("only for build scripts");
    // OUT_DIR is target/<profile>/build/<crate>-<hash>/out (or with the target triple
    // before the profile when cross-compiling); OCCT goes next to the profiles.
    let mut dir = Path::new(&out).to_path_buf();
    for _ in 0..4 {
        dir.pop();
    }
    dir.join(format!("OCCT-{OCCT_VERSION}"))
}

/// Configures, builds and installs OCCT into [`occt_path`]. CMake skips the work when
/// nothing changed.
pub fn build_occt() {
    let off = [
        "BUILD_MODULE_Draw",
        "BUILD_MODULE_Visualization",
        "BUILD_DOC_Overview",
        "BUILD_DOC_RefMan",
        "BUILD_GTEST",
        "BUILD_USE_PCH",
        "BUILD_USE_VCPKG",
        "USE_D3D",
        "USE_DRACO",
        "USE_EIGEN",
        "USE_FFMPEG",
        "USE_FREEIMAGE",
        "USE_FREETYPE",
        "USE_GLES2",
        "USE_OPENGL",
        "USE_OPENVR",
        "USE_RAPIDJSON",
        "USE_TBB",
        "USE_TCL",
        "USE_TK",
        "USE_VTK",
        "USE_XLIB",
    ];
    let on = [
        "BUILD_MODULE_FoundationClasses",
        "BUILD_MODULE_ModelingData",
        "BUILD_MODULE_ModelingAlgorithms",
        "BUILD_MODULE_ApplicationFramework",
        "BUILD_MODULE_DataExchange",
    ];
    let mut config = cmake::Config::new(Path::new(env!("OCCT_SRC_DIR")));
    for option in off {
        config.define(option, "OFF");
    }
    for option in on {
        config.define(option, "ON");
    }
    config
        .define("BUILD_LIBRARY_TYPE", "Static")
        .define("INSTALL_DIR_LIB", LIB_DIR)
        .define("INSTALL_DIR_INCLUDE", INCLUDE_DIR)
        .profile("Release")
        .out_dir(occt_path())
        .build();
}
