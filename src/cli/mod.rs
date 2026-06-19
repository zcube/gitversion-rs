//! clap-based command-line interface.
//!
//! Ports the main options from the original `GitVersion.App/ArgumentParser.cs`.

use crate::config::{
    DeploymentMode, GitVersionConfiguration, IncrementStrategy, SemanticVersionFormat,
};
use clap::{CommandFactory, Parser, ValueEnum};
use rust_i18n::t;
use std::path::PathBuf;

/// Output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    Json,
    /// Corresponds to the original `/output file`: writes JSON to `--outputfile` (same rendering as Json).
    File,
    DotEnv,
    BuildServer,
}

/// Log verbosity level. Corresponds to the original `Verbosity`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Verbosity {
    Quiet,
    Minimal,
    Normal,
    Verbose,
    Diagnostic,
}

impl Verbosity {
    /// Convert to a `log` crate level filter.
    pub fn to_level(self) -> log::LevelFilter {
        match self {
            Verbosity::Quiet => log::LevelFilter::Error,
            Verbosity::Minimal => log::LevelFilter::Warn,
            Verbosity::Normal => log::LevelFilter::Info,
            Verbosity::Verbose => log::LevelFilter::Debug,
            Verbosity::Diagnostic => log::LevelFilter::Trace,
        }
    }
}

// Help/about strings default to English in source; `localized_command()` overrides them at
// runtime using `cli.about` / `cli.help.<id>` keys. Struct doc comments (`///`) become
// `long_about` which shadows the `about` override, so plain comments (`//`) are used here.
#[derive(Debug, Parser)]
#[command(
    name = "gitversion-rs",
    version,
    about = "Calculate a semantic version from Git history (GitVersion Rust port)"
)]
pub struct Cli {
    /// Repository path (directory containing `.git`). Defaults to the current directory.
    #[arg(default_value = ".")]
    pub path: PathBuf,

    /// Same as path but position-independent (upstream `/targetpath`).
    #[arg(long = "targetpath", value_name = "DIR")]
    pub target_path: Option<PathBuf>,

    // nofetch/nonormalize/allowshallow are accepted for CLI compatibility with the original,
    // but have no effect in this port because fetch/normalize are not performed.
    /// Disable fetch (no-op: this port does not fetch).
    #[arg(long)]
    pub nofetch: bool,
    /// Disable normalization (no-op).
    #[arg(long)]
    pub nonormalize: bool,
    /// Disable disk cache read/write (`<.git>/gitversion_cache`).
    #[arg(long)]
    pub nocache: bool,
    /// Allow shallow clone (no-op: gix reads shallow repos too).
    #[arg(long)]
    pub allowshallow: bool,

    /// Output format (json, dot-env, build-server). May be repeated.
    #[arg(long, value_enum, default_value = "json")]
    pub output: Vec<OutputFormat>,

    /// Output file path (writes the result to a file when set).
    #[arg(long = "outputfile")]
    pub output_file: Option<PathBuf>,

    /// Print a single variable only (e.g. -v SemVer).
    #[arg(long = "showvariable", short = 'v')]
    pub show_variable: Option<String>,

    /// Print using a format string (e.g. --format "{Major}.{Minor}").
    #[arg(long)]
    pub format: Option<String>,

    /// Config file path.
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Print the effective config as YAML and exit.
    #[arg(long = "showconfig")]
    pub show_config: bool,

    /// Inline config override (key=value). May be repeated.
    #[arg(long = "overrideconfig")]
    pub override_config: Vec<String>,

    /// Branch to compute for (instead of the current checkout).
    #[arg(long, short = 'b')]
    pub branch: Option<String>,

    /// Output language (ko/en/ja/zh). Falls back to LANG/LC_ALL when omitted.
    #[arg(long, value_name = "LANG")]
    pub lang: Option<String>,

    /// Log verbosity.
    #[arg(long, value_enum, default_value = "normal")]
    pub verbosity: Verbosity,

    /// Write log output to a file (upstream `/l`), or `console` for stderr.
    /// Logs append; stdout stays clean.
    #[arg(long = "log", short = 'l', value_name = "FILE")]
    pub log_file: Option<PathBuf>,

    /// Diagnostic mode (Trace logging).
    #[arg(long)]
    pub diag: bool,

    /// Update AssemblyInfo files (recursive search when no file is given).
    #[arg(long = "updateassemblyinfo", num_args = 0.., value_name = "FILE")]
    pub update_assembly_info: Option<Vec<String>>,

    /// Create the AssemblyInfo file if missing (with updateassemblyinfo).
    #[arg(long = "ensureassemblyinfo")]
    pub ensure_assembly_info: bool,

    /// Update version elements in project files (.csproj etc.; recursive when no file is given).
    #[arg(long = "updateprojectfiles", num_args = 0.., value_name = "FILE")]
    pub update_project_files: Option<Vec<String>>,

    /// Create GitVersion_WixVersion.wxi.
    #[arg(long = "updatewixversionfile")]
    pub update_wix_version_file: bool,

    /// Update the version in package manifests (package.json/Cargo.toml/pyproject.toml).
    /// Recursive search when no file is given.
    #[arg(long = "updatepackagefiles", num_args = 0.., value_name = "FILE")]
    pub update_package_files: Option<Vec<String>>,

    /// Remote git repository URL (clone then compute when set). Requires `--branch`.
    #[arg(long)]
    pub url: Option<String>,

    /// Remote auth username (with `--url`).
    #[arg(long = "username", short = 'u')]
    pub username: Option<String>,

    /// Remote auth password (with `--url`).
    #[arg(long = "password", short = 'p')]
    pub password: Option<String>,

    /// Commit ID to inspect (latest on the branch when omitted). With `--url`.
    #[arg(long = "commit", short = 'c')]
    pub commit: Option<String>,

    /// Dynamic clone location (default: a temp directory).
    #[arg(long = "dynamicRepoLocation")]
    pub dynamic_repo_location: Option<PathBuf>,

    /// prepare command to run after computing (version variables exposed as GitVersion_* env and {Var}).
    #[arg(long)]
    pub exec: Option<String>,

    /// Version-modifying command. Its stdout is applied as next-version and recomputed.
    #[arg(long = "exec-version")]
    pub exec_version: Option<String>,

    /// Print exec hooks without actually running them.
    #[arg(long = "dry-run")]
    pub dry_run: bool,

    /// Launch the interactive Ratatui TUI.
    #[arg(long)]
    pub tui: bool,
}

/// Build a clap Command with `about` and per-argument `help` overridden by `t!` for the current locale.
/// If a `cli.about` or `cli.help.<arg_id>` key resolves to a translated string, it replaces the
/// English source doc; if the key is returned as-is (no translation), the English source doc is kept.
/// Must be called after the locale is set (before argument parsing).
pub fn localized_command() -> clap::Command {
    Cli::command()
        .about(t!("cli.about").to_string())
        .mut_args(|arg| {
            let key = format!("cli.help.{}", arg.get_id());
            let val = t!(key.as_str()).to_string();
            if val == key {
                arg
            } else {
                arg.help(val)
            }
        })
}

/// Apply `key=value` overrideconfig entries to the configuration.
pub fn apply_overrides(config: &mut GitVersionConfiguration, overrides: &[String]) {
    for raw in overrides {
        let Some((key, value)) = raw.split_once('=') else {
            log::warn!("{}", t!("cli.override_invalid", entry = raw));
            continue;
        };
        let key = key.trim();
        let value = value.trim().to_string();
        match key {
            "tag-prefix" => config.tag_prefix = Some(value),
            "next-version" => config.next_version = Some(value),
            "label" => config.label = Some(value),
            "commit-date-format" => config.commit_date_format = Some(value),
            "major-version-bump-message" => config.major_version_bump_message = Some(value),
            "minor-version-bump-message" => config.minor_version_bump_message = Some(value),
            "patch-version-bump-message" => config.patch_version_bump_message = Some(value),
            "no-bump-message" => config.no_bump_message = Some(value),
            "tag-pre-release-weight" => {
                if let Ok(n) = value.parse() {
                    config.tag_pre_release_weight = Some(n);
                }
            }
            "update-build-number" => config.update_build_number = value.parse().ok(),
            "increment" => config.increment = parse_increment(&value),
            "mode" => config.mode = parse_mode(&value),
            "semantic-version-format" => {
                config.semantic_version_format = match value.to_lowercase().as_str() {
                    "loose" => Some(SemanticVersionFormat::Loose),
                    _ => Some(SemanticVersionFormat::Strict),
                }
            }
            other => log::warn!("{}", t!("cli.override_unsupported", key = other)),
        }
    }
}

fn parse_increment(v: &str) -> Option<IncrementStrategy> {
    Some(match v.to_lowercase().as_str() {
        "major" => IncrementStrategy::Major,
        "minor" => IncrementStrategy::Minor,
        "patch" => IncrementStrategy::Patch,
        "none" => IncrementStrategy::None,
        "inherit" => IncrementStrategy::Inherit,
        _ => return None,
    })
}

fn parse_mode(v: &str) -> Option<DeploymentMode> {
    Some(match v.to_lowercase().as_str() {
        "continuousdelivery" => DeploymentMode::ContinuousDelivery,
        "continuousdeployment" => DeploymentMode::ContinuousDeployment,
        "manualdeployment" => DeploymentMode::ManualDeployment,
        _ => return None,
    })
}
