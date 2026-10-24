//! Tells the library where the OCCT sources are, so the build script of tp-cad can
//! build them.

fn main() {
    let dir = std::env::var("CARGO_MANIFEST_DIR").expect("set by cargo");
    let source = std::path::Path::new(&dir).join("OCCT");
    assert!(
        source.join("CMakeLists.txt").exists(),
        "the OCCT sources are missing in {}: run `git submodule update --init`",
        source.display()
    );
    println!("cargo:rustc-env=OCCT_SRC_DIR={}", source.display());
    println!("cargo:rerun-if-changed=build.rs");
}
