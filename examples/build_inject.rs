//! Build-time version injection using gitversion-rs **as a library**.
//!
//! This mirrors what you would put in a real project's `build.rs`: compute the
//! version from Git history with gitversion-rs and forward it to the compiler via
//! `cargo:rustc-env=` so the crate can read it at compile time with `env!(...)`.
//!
//! Run it from the repository root to see the lines a `build.rs` would emit:
//!
//!     cargo run --example build_inject
//!
//! ## Using it in your own project
//!
//! Add gitversion-rs as a **build-dependency** and put the logic in `build.rs`:
//!
//! ```toml
//! # Cargo.toml
//! [build-dependencies]
//! gitversion-rs = "0.1"
//! ```
//!
//! ```rust,ignore
//! // build.rs
//! use std::path::Path;
//! use gitversion_rs::{config, git, version};
//!
//! fn main() -> anyhow::Result<()> {
//!     let work_dir = Path::new(".").canonicalize()?;
//!     let repo = git::GitRepo::discover(&work_dir)?;
//!     let repo_root = repo.workdir().map(Path::to_path_buf);
//!     let cfg = config::loader::load(None, &work_dir, repo_root.as_deref())?;
//!     let vars = version::calculation::calculate(&repo, &cfg, None)?;
//!
//!     // Overwrite Cargo's CARGO_PKG_VERSION* with the GitVersion-computed values.
//!     println!("cargo:rustc-env=CARGO_PKG_VERSION={}", vars.sem_ver);
//!     println!("cargo:rustc-env=GITVERSION_FULL_SEMVER={}", vars.full_sem_ver);
//!     // Recompute whenever HEAD moves.
//!     println!("cargo:rerun-if-changed=.git/HEAD");
//!     Ok(())
//! }
//! ```
//!
//! Then read the injected values anywhere in your crate:
//!
//! ```rust,ignore
//! const VERSION: &str = env!("CARGO_PKG_VERSION");        // overwritten by build.rs
//! const FULL_SEMVER: &str = env!("GITVERSION_FULL_SEMVER");
//! ```
//!
//! Unlike the exec-hook approach (see `examples/exec-config-inject/`), this can
//! override the crate's own `CARGO_PKG_VERSION` because `cargo:rustc-env` is applied
//! by Cargo itself, after it reads `Cargo.toml`.

use std::path::Path;

use gitversion_rs::{config, git, version};

fn main() -> anyhow::Result<()> {
    // 1. Open the repository (here: the current working directory).
    let work_dir = Path::new(".").canonicalize()?;
    let repo = git::GitRepo::discover(&work_dir)?;
    let repo_root = repo.workdir().map(Path::to_path_buf);

    // 2. Load GitVersion.yml (falls back to GitFlow defaults when absent).
    let configuration = config::loader::load(None, &work_dir, repo_root.as_deref())?;

    // 3. Calculate the version variables from Git history.
    let vars = version::calculation::calculate(&repo, &configuration, None)?;

    // 4. Emit the `cargo:rustc-env=` lines a real build.rs would produce.
    //    The CARGO_PKG_VERSION* names match what Cargo sets, so they transparently
    //    override the placeholder version in Cargo.toml.
    println!("cargo:rustc-env=CARGO_PKG_VERSION={}", vars.sem_ver);
    println!("cargo:rustc-env=CARGO_PKG_VERSION_MAJOR={}", vars.major);
    println!("cargo:rustc-env=CARGO_PKG_VERSION_MINOR={}", vars.minor);
    println!("cargo:rustc-env=CARGO_PKG_VERSION_PATCH={}", vars.patch);
    println!(
        "cargo:rustc-env=CARGO_PKG_VERSION_PRE={}",
        vars.pre_release_tag
    );

    // The richer GitVersion variables, under a GITVERSION_ prefix.
    println!(
        "cargo:rustc-env=GITVERSION_FULL_SEMVER={}",
        vars.full_sem_ver
    );
    println!(
        "cargo:rustc-env=GITVERSION_INFORMATIONAL_VERSION={}",
        vars.informational_version
    );
    println!("cargo:rustc-env=GITVERSION_SHA={}", vars.sha);

    // Trigger a rebuild whenever HEAD changes so the version stays current.
    println!("cargo:rerun-if-changed=.git/HEAD");

    Ok(())
}
