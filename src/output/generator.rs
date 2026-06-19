//! Output formatters: JSON, dotenv, single variable, format string, AssemblyInfo.
//!
//! Corresponds to `GitVersion.Output/OutputGenerator` in the original.

use super::variables::VersionVariables;
use anyhow::{bail, Result};
use regex::Regex;
use rust_i18n::t;

/// JSON output (PascalCase keys matching the original, pretty-printed).
pub fn to_json(vars: &VersionVariables) -> Result<String> {
    Ok(serde_json::to_string_pretty(vars)?)
}

/// dotenv output: `GitVersion_Major='1'` format.
pub fn to_dotenv(vars: &VersionVariables) -> String {
    let mut out = String::new();
    for (k, v) in vars.to_map() {
        out.push_str(&format!("GitVersion_{k}='{v}'\n"));
    }
    out
}

/// Output a single variable value. Returns an error if the variable does not exist.
pub fn show_variable(vars: &VersionVariables, name: &str) -> Result<String> {
    let map = vars.to_map();
    match map.get(name) {
        Some(v) => Ok(v.clone()),
        None => {
            let known: Vec<_> = map.keys().cloned().collect();
            bail!(
                "{}",
                t!(
                    "output.unknown_variable",
                    name = name,
                    known = known.join(", ")
                )
            )
        }
    }
}

/// Substitute `{Variable}` tokens in a format string. `{env:VAR}` is also supported.
pub fn format_template(vars: &VersionVariables, template: &str) -> Result<String> {
    let map = vars.to_map();
    let re = Regex::new(r"\{(?<token>[A-Za-z0-9_:]+)\}").unwrap();
    let mut missing: Option<String> = None;
    let result = re
        .replace_all(template, |caps: &regex::Captures| {
            let token = &caps["token"];
            if let Some(env_var) = token.strip_prefix("env:") {
                std::env::var(env_var).unwrap_or_default()
            } else if let Some(v) = map.get(token) {
                v.clone()
            } else {
                missing.get_or_insert_with(|| token.to_string());
                String::new()
            }
        })
        .into_owned();
    if let Some(m) = missing {
        bail!("{}", t!("output.unknown_token", token = m));
    }
    Ok(result)
}

/// Environment-variable export lines for build servers (GitHub Actions and similar).
pub fn to_buildserver_env(vars: &VersionVariables) -> String {
    let mut out = String::new();
    for (k, v) in vars.to_map() {
        out.push_str(&format!("GitVersion_{k}={v}\n"));
    }
    out
}
