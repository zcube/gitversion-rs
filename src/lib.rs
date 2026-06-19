//! Library surface of gitversion-rs (Rust port of GitVersion).
//!
//! Exposes modules shared by the binary (`main.rs`) and integration tests (`tests/`).

// Some helpers (e.g. tags_on_commit, format_short) are intentionally included even though
// the binary does not call them directly, as they are part of the ported public API.
#![allow(dead_code)]

// Multilingual: English by default; ko/ja/zh loaded from YAML files in locales/.
rust_i18n::i18n!("locales", fallback = "en");

pub mod i18n;

pub mod app;
pub mod buildagent;
pub mod cache;
pub mod cli;
pub mod config;
pub mod exec;
pub mod git;
pub mod output;
pub mod remote;
pub mod tui;
pub mod version;
