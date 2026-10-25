//! Builds OCCT (via tp-occt-sys) and the C++ bridge of tp-cad, and links them statically.

/// OCCT toolkits used by the bridge, from high to low level, so single-pass linkers
/// (GNU ld) resolve every symbol. The bridge creates OCCT's type registry in its own
/// static initializer, so the order does not affect destruction at exit.
const OCCT_LIBS: &[&str] = &[
    "TKDESTEP",
    "TKDEIGES",
    "TKDESTL",
    "TKXCAF",
    "TKCAF",
    "TKLCAF",
    "TKCDF",
    "TKXSBase",
    "TKDE",
    "TKFeat",
    "TKFillet",
    "TKOffset",
    "TKBool",
    "TKBO",
    "TKMesh",
    "TKShHealing",
    "TKPrim",
    "TKTopAlgo",
    "TKGeomAlgo",
    "TKBRep",
    "TKGeomBase",
    "TKG3d",
    "TKG2d",
    "TKMath",
    "TKernel",
];

fn main() {
    tp_occt_sys::build_occt();
    let occt = tp_occt_sys::occt_path();

    cxx_build::bridge("src/bridge.rs")
        .file("cpp/tp_cad.cpp")
        .include(occt.join("include"))
        .std("c++17")
        .define("_USE_MATH_DEFINES", None)
        .flag_if_supported("-Wno-deprecated-declarations")
        .flag_if_supported("-Wno-unused-parameter")
        .compile("tp-cad-bridge");

    println!(
        "cargo:rustc-link-search=native={}",
        occt.join("lib").display()
    );
    for lib in OCCT_LIBS {
        println!("cargo:rustc-link-lib=static={lib}");
    }
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.contains("windows") {
        println!("cargo:rustc-link-lib=dylib=user32");
        // TKernel's OSD layer uses the Win32 security and registry API.
        println!("cargo:rustc-link-lib=dylib=advapi32");
    } else if target.contains("linux") {
        println!("cargo:rustc-link-lib=dylib=pthread");
        println!("cargo:rustc-link-lib=dylib=dl");
    }
    for path in ["src/bridge.rs", "cpp/tp_cad.cpp", "cpp/tp_cad.hxx"] {
        println!("cargo:rerun-if-changed={path}");
    }
}
