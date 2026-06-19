//! Sample `build.rs` for the consuming crate.
//!
//! Reads the version that was stamped into Cargo.toml (and that Cargo therefore
//! exposes as `CARGO_PKG_VERSION`) and forwards it to the compiler as an extra
//! compile-time constant the application can read with `env!("APP_VERSION")`.
//!
//! This file is a documentation sample — it is intentionally NOT named `main.rs`
//! so Cargo does not auto-discover it as an example of the gitversion-rs crate.

fn main() {
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    println!("cargo:rustc-env=APP_VERSION={version}");
}
