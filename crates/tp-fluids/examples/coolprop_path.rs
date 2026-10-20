//! Prints the path of the CoolProp library built for this platform by the
//! `coolprop-sys` dev-dependency, for use in development:
//!
//! ```bash
//! export TURBOPUMP_COOLPROP="$(cargo run -q -p tp-fluids --example coolprop_path)"
//! ```

fn main() {
    println!("{}", coolprop_sys::COOLPROP_PATH);
}
