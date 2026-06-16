//! 출력 포맷터: JSON, dotenv, 단일 변수, 포맷 문자열, AssemblyInfo.
//!
//! 원본 `GitVersion.Output/OutputGenerator` 대응.

use super::variables::VersionVariables;
use anyhow::{bail, Result};
use regex::Regex;
use rust_i18n::t;

/// JSON 출력(원본과 동일한 PascalCase 키, pretty).
pub fn to_json(vars: &VersionVariables) -> Result<String> {
    Ok(serde_json::to_string_pretty(vars)?)
}

/// dotenv 출력: `GitVersion_Major='1'` 형식.
pub fn to_dotenv(vars: &VersionVariables) -> String {
    let mut out = String::new();
    for (k, v) in vars.to_map() {
        out.push_str(&format!("GitVersion_{k}='{v}'\n"));
    }
    out
}

/// 단일 변수 값 출력. 존재하지 않으면 에러.
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

/// 포맷 문자열의 `{Variable}` 치환. `{env:VAR}` 도 지원.
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

/// 빌드서버용 환경변수 export 라인(GitHub Actions 등 공통 형식).
pub fn to_buildserver_env(vars: &VersionVariables) -> String {
    let mut out = String::new();
    for (k, v) in vars.to_map() {
        out.push_str(&format!("GitVersion_{k}={v}\n"));
    }
    out
}
