//! 외부 명령 실행 훅(semantic-release exec 플러그인 유사).
//!
//! 계산된 버전 변수를 `GitVersion_*` 환경변수와 `{Variable}`/`{env:VAR}` 템플릿으로
//! 노출하고, 라이프사이클 훅 명령을 실행한다. `version` 훅은 명령의 표준출력으로
//! 버전 정보를 수정(next-version 덮어쓰기 후 재계산)할 수 있다.

use crate::output::VersionVariables;
use anyhow::{bail, Context, Result};
use regex::Regex;
use rust_i18n::t;
use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, Stdio};

/// side-effect 훅 실행 순서.
pub const HOOK_ORDER: [&str; 4] = ["verify", "prepare", "publish", "success"];

/// 명령 문자열의 `{Variable}` / `{env:VAR}` 토큰을 치환(미지의 토큰은 그대로 둠).
fn render(cmd: &str, map: &BTreeMap<String, String>) -> String {
    let re = Regex::new(r"\{(?<t>[A-Za-z0-9_:]+)\}").unwrap();
    re.replace_all(cmd, |c: &regex::Captures| {
        let t = &c["t"];
        if let Some(env_var) = t.strip_prefix("env:") {
            std::env::var(env_var).unwrap_or_default()
        } else if let Some(v) = map.get(t) {
            v.clone()
        } else {
            format!("{{{t}}}") // 미지의 토큰은 보존.
        }
    })
    .into_owned()
}

/// 버전 변수를 `GitVersion_*` 환경변수로 변환.
fn env_vars(vars: &VersionVariables) -> Vec<(String, String)> {
    vars.to_map()
        .into_iter()
        .map(|(k, v)| (format!("GitVersion_{k}"), v))
        .collect()
}

/// 쉘로 명령을 실행. `capture` 면 stdout 을 수집해 반환, 아니면 상속 출력.
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
        .envs(env_vars(vars));
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

/// `version` 훅(또는 --exec-version) 실행. stdout 의 첫 비어있지 않은 줄을 반환.
/// 그 값은 호출자가 next-version 으로 적용해 재계산한다.
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

/// side-effect 훅(verify/prepare/publish/success)을 순서대로 실행.
/// 실패 시 `fail` 훅이 있으면 실행하고 에러를 전파한다.
/// `extra_prepare` 는 --exec 로 준 임시 prepare 명령(설정 prepare 다음에 실행).
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
        // 미지의 토큰은 보존.
        assert_eq!(render("echo {Unknown}", &m), "echo {Unknown}");
        // 쉘 변수($)는 영향 없음.
        assert_eq!(render("echo $HOME {SemVer}", &m), "echo $HOME 1.2.3");
    }
}
