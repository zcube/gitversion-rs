//! External command execution hooks (similar to the semantic-release exec plugin).
//!
//! Exposes computed version variables as `GitVersion_*` environment variables and
//! `{Variable}`/`{env:VAR}` template tokens, then runs lifecycle hook commands.
//! The `version` hook can modify the version by writing to stdout
//! (which overwrites `next-version` and triggers a recalculation).

use crate::output::VersionVariables;
use anyhow::{bail, Context, Result};
use regex::Regex;
use rust_i18n::t;
use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, Stdio};

/// Execution order for side-effect hooks.
pub const HOOK_ORDER: [&str; 4] = ["verify", "prepare", "publish", "success"];

/// Substitute `{Variable}` / `{env:VAR}` tokens in a command string (unknown tokens are left as-is).
fn render(cmd: &str, map: &BTreeMap<String, String>) -> String {
    let re = Regex::new(r"\{(?<t>[A-Za-z0-9_:]+)\}").unwrap();
    re.replace_all(cmd, |c: &regex::Captures| {
        let t = &c["t"];
        if let Some(env_var) = t.strip_prefix("env:") {
            std::env::var(env_var).unwrap_or_default()
        } else if let Some(v) = map.get(t) {
            v.clone()
        } else {
            format!("{{{t}}}") // Unknown tokens are preserved as-is.
        }
    })
    .into_owned()
}

/// Convert version variables to `GitVersion_*` environment variable pairs.
fn env_vars(vars: &VersionVariables) -> Vec<(String, String)> {
    vars.to_map()
        .into_iter()
        .map(|(k, v)| (format!("GitVersion_{k}"), v))
        .collect()
}

/// Standard Cargo `CARGO_PKG_VERSION*` environment variables derived from the version.
///
/// These mirror the names Cargo itself sets at build time, so a Rust build or script
/// invoked from an exec hook can pick up the GitVersion-computed version using the
/// familiar variable names (e.g. a `build.rs` reading `CARGO_PKG_VERSION`).
fn cargo_env_vars(vars: &VersionVariables) -> Vec<(String, String)> {
    vec![
        ("CARGO_PKG_VERSION".into(), vars.sem_ver.clone()),
        ("CARGO_PKG_VERSION_MAJOR".into(), vars.major.to_string()),
        ("CARGO_PKG_VERSION_MINOR".into(), vars.minor.to_string()),
        ("CARGO_PKG_VERSION_PATCH".into(), vars.patch.to_string()),
        ("CARGO_PKG_VERSION_PRE".into(), vars.pre_release_tag.clone()),
    ]
}

/// Run a command via the shell. If `capture` is true, collect stdout and return it; otherwise inherit.
fn run_command(
    cmd: &str,
    vars: &VersionVariables,
    work_dir: &Path,
    capture: bool,
    dry_run: bool,
) -> Result<Option<String>> {
    let rendered = render(cmd, &vars.to_map());
    if dry_run {
        log::info!("{}", t!("exec.dry_run", cmd = rendered));
        eprintln!("[dry-run] {rendered}");
        return Ok(None);
    }
    log::info!("{}", t!("exec.running", cmd = rendered));

    let (program, flag) = if cfg!(windows) {
        ("cmd", "/C")
    } else {
        ("sh", "-c")
    };
    let mut command = Command::new(program);
    command
        .arg(flag)
        .arg(&rendered)
        .current_dir(work_dir)
        .envs(env_vars(vars))
        .envs(cargo_env_vars(vars));
    if capture {
        command.stdout(Stdio::piped()).stderr(Stdio::inherit());
    }

    if capture {
        let output = command
            .output()
            .with_context(|| t!("exec.run_failed", cmd = rendered))?;
        if !output.status.success() {
            bail!(
                "{}",
                t!(
                    "exec.cmd_failed",
                    code = format!("{:?}", output.status.code()),
                    cmd = rendered
                )
            );
        }
        Ok(Some(String::from_utf8_lossy(&output.stdout).into_owned()))
    } else {
        let status = command
            .status()
            .with_context(|| t!("exec.run_failed", cmd = rendered))?;
        if !status.success() {
            bail!(
                "{}",
                t!(
                    "exec.cmd_failed",
                    code = format!("{:?}", status.code()),
                    cmd = rendered
                )
            );
        }
        Ok(None)
    }
}

/// Run the `version` hook (or `--exec-version`). Returns the first non-empty line from stdout.
/// The caller applies the result as `next-version` and recalculates.
pub fn run_version_hook(
    cmd: &str,
    vars: &VersionVariables,
    work_dir: &Path,
    dry_run: bool,
) -> Result<Option<String>> {
    let out = run_command(cmd, vars, work_dir, true, dry_run)?;
    Ok(out.and_then(|s| {
        s.lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .map(String::from)
    }))
}

/// Run side-effect hooks (verify/prepare/publish/success) in order.
/// On failure, runs the `fail` hook if present and then propagates the error.
/// `extra_prepare` is the temporary prepare command supplied via `--exec` (run after the config's prepare).
pub fn run_hooks(
    hooks: &BTreeMap<String, String>,
    extra_prepare: Option<&str>,
    vars: &VersionVariables,
    work_dir: &Path,
    dry_run: bool,
) -> Result<()> {
    let mut result = Ok(());
    'outer: for &name in &HOOK_ORDER {
        if let Some(cmd) = hooks.get(name) {
            if let Err(e) = run_command(cmd, vars, work_dir, false, dry_run) {
                result = Err(e.context(t!("exec.hook_failed", name = name).to_string()));
                break 'outer;
            }
        }
        if name == "prepare" {
            if let Some(cmd) = extra_prepare {
                if let Err(e) = run_command(cmd, vars, work_dir, false, dry_run) {
                    result = Err(e.context(t!("exec.exec_prepare_failed").to_string()));
                    break 'outer;
                }
            }
        }
    }

    if result.is_err() {
        if let Some(fail_cmd) = hooks.get("fail") {
            log::warn!("{}", t!("exec.running_fail_hook"));
            let _ = run_command(fail_cmd, vars, work_dir, false, dry_run);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_substitutes_and_preserves() {
        let mut m = BTreeMap::new();
        m.insert("SemVer".to_string(), "1.2.3".to_string());
        assert_eq!(render("echo {SemVer}", &m), "echo 1.2.3");
        // Unknown tokens are preserved.
        assert_eq!(render("echo {Unknown}", &m), "echo {Unknown}");
        // Shell variables ($) are not affected.
        assert_eq!(render("echo $HOME {SemVer}", &m), "echo $HOME 1.2.3");
    }

    #[test]
    fn cargo_env_vars_mirror_cargo_names() {
        let vars = VersionVariables {
            major: 1,
            minor: 2,
            patch: 3,
            sem_ver: "1.2.3-alpha.4".into(),
            pre_release_tag: "alpha.4".into(),
            ..Default::default()
        };
        let map: BTreeMap<_, _> = cargo_env_vars(&vars).into_iter().collect();
        assert_eq!(map["CARGO_PKG_VERSION"], "1.2.3-alpha.4");
        assert_eq!(map["CARGO_PKG_VERSION_MAJOR"], "1");
        assert_eq!(map["CARGO_PKG_VERSION_MINOR"], "2");
        assert_eq!(map["CARGO_PKG_VERSION_PATCH"], "3");
        assert_eq!(map["CARGO_PKG_VERSION_PRE"], "alpha.4");
    }
}
