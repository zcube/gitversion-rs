//! 빌드에이전트(CI) 통합.
//!
//! 원본 `GitVersion.BuildAgents` 의 각 에이전트를 옮긴다. 환경변수로 현재 CI 를
//! 감지하고, 변수/빌드번호를 해당 CI 의 형식으로 출력한다. `update_build_number`
//! 가 false 면 빌드번호 설정을 생략한다(원본 `BuildAgentBase.WriteIntegration`).

use crate::output::VersionVariables;
use std::env;

/// TeamCity/MyGet service message 값 이스케이프.
fn escape_value(v: &str) -> String {
    v.replace('|', "||")
        .replace('\'', "|'")
        .replace('[', "|[")
        .replace(']', "|]")
        .replace('\r', "|r")
        .replace('\n', "|n")
}

/// 빌드에이전트 공통 인터페이스.
pub trait BuildAgent {
    /// 원본 클래스명(GetType().Name)과 동일.
    fn name(&self) -> &'static str;

    /// 빌드번호 설정 출력(대부분 FullSemVer). 없는 CI 는 빈 문자열.
    fn set_build_number(&self, vars: &VersionVariables) -> String {
        vars.full_sem_ver.clone()
    }

    /// 단일 변수 출력 라인들.
    fn set_output_variable(&self, name: &str, value: &str) -> Vec<String>;

    /// 전체 통합 출력(로그 라인 포함).
    fn write_integration(&self, vars: &VersionVariables, update_build_number: bool) -> Vec<String> {
        base_integration(self, vars, update_build_number)
    }
}

/// 기본 WriteIntegration 동작(원본 BuildAgentBase).
fn base_integration(
    agent: &(impl BuildAgent + ?Sized),
    vars: &VersionVariables,
    update_build_number: bool,
) -> Vec<String> {
    let mut out = Vec::new();
    if update_build_number {
        out.push(format!("Set Build Number for '{}'.", agent.name()));
        out.push(agent.set_build_number(vars));
    }
    out.push(format!("Set Output Variables for '{}'.", agent.name()));
    for (key, value) in vars.to_map() {
        out.extend(agent.set_output_variable(&key, &value));
    }
    out
}

// ─────────────────────────── 에이전트 구현 ───────────────────────────

/// TeamCity: `##teamcity[...]` service message.
struct TeamCity;
impl BuildAgent for TeamCity {
    fn name(&self) -> &'static str {
        "TeamCity"
    }
    fn set_build_number(&self, vars: &VersionVariables) -> String {
        format!("##teamcity[buildNumber '{}']", escape_value(&vars.full_sem_ver))
    }
    fn set_output_variable(&self, name: &str, value: &str) -> Vec<String> {
        let e = escape_value(value);
        vec![
            format!("##teamcity[setParameter name='GitVersion.{name}' value='{e}']"),
            format!("##teamcity[setParameter name='system.GitVersion.{name}' value='{e}']"),
        ]
    }
}

/// MyGet: `##myget[...]`.
struct MyGet;
impl BuildAgent for MyGet {
    fn name(&self) -> &'static str {
        "MyGet"
    }
    fn set_build_number(&self, vars: &VersionVariables) -> String {
        format!("##myget[buildNumber '{}']", escape_value(&vars.full_sem_ver))
    }
    fn set_output_variable(&self, name: &str, value: &str) -> Vec<String> {
        vec![format!("##myget[setParameter name='GitVersion.{name}' value='{}']", escape_value(value))]
    }
}

/// Azure Pipelines: `##vso[...]`.
struct AzurePipelines;
impl BuildAgent for AzurePipelines {
    fn name(&self) -> &'static str {
        "AzurePipelines"
    }
    fn set_build_number(&self, vars: &VersionVariables) -> String {
        // BUILD_BUILDNUMBER 가 없으면 FullSemVer(+0 접미사 제거).
        match env::var("BUILD_BUILDNUMBER") {
            Ok(bn) if !bn.trim().is_empty() => {
                let replaced = replace_azure_vars(&bn, vars);
                if replaced != bn {
                    format!("##vso[build.updatebuildnumber]{replaced}")
                } else {
                    let v = vars.full_sem_ver.strip_suffix("+0").unwrap_or(&vars.full_sem_ver);
                    format!("##vso[build.updatebuildnumber]{v}")
                }
            }
            _ => vars.full_sem_ver.clone(),
        }
    }
    fn set_output_variable(&self, name: &str, value: &str) -> Vec<String> {
        vec![
            format!("##vso[task.setvariable variable=GitVersion.{name}]{value}"),
            format!("##vso[task.setvariable variable=GitVersion.{name};isOutput=true]{value}"),
        ]
    }
}

fn replace_azure_vars(build_number: &str, vars: &VersionVariables) -> String {
    let mut out = build_number.to_string();
    for (key, value) in vars.to_map() {
        out = out.replace(&format!("$(GITVERSION_{key})"), &value);
        out = out.replace(&format!("$(GITVERSION.{key})"), &value);
    }
    out
}

/// ContinuaCI: `@@continua[...]`.
struct ContinuaCi;
impl BuildAgent for ContinuaCi {
    fn name(&self) -> &'static str {
        "ContinuaCi"
    }
    fn set_build_number(&self, vars: &VersionVariables) -> String {
        format!("@@continua[setBuildVersion value='{}']", vars.full_sem_ver)
    }
    fn set_output_variable(&self, name: &str, value: &str) -> Vec<String> {
        vec![format!(
            "@@continua[setVariable name='GitVersion_{name}' value='{value}' skipIfNotDefined='true']"
        )]
    }
}

/// EnvRun: `@@envrun[...]`.
struct EnvRun;
impl BuildAgent for EnvRun {
    fn name(&self) -> &'static str {
        "EnvRun"
    }
    fn set_output_variable(&self, name: &str, value: &str) -> Vec<String> {
        vec![format!("@@envrun[set name='GitVersion_{name}' value='{value}']")]
    }
}

/// `GitVersion_{name}={value}` 형식 공통(TravisCI, Drone, GitLabCi, Jenkins, CodeBuild).
fn key_value_line(name: &str, value: &str) -> Vec<String> {
    vec![format!("GitVersion_{name}={value}")]
}

struct TravisCi;
impl BuildAgent for TravisCi {
    fn name(&self) -> &'static str {
        "TravisCI"
    }
    fn set_output_variable(&self, name: &str, value: &str) -> Vec<String> {
        key_value_line(name, value)
    }
}

struct Drone;
impl BuildAgent for Drone {
    fn name(&self) -> &'static str {
        "Drone"
    }
    fn set_output_variable(&self, name: &str, value: &str) -> Vec<String> {
        key_value_line(name, value)
    }
}

/// gitversion.properties 파일에 변수 기록(GitLabCi, Jenkins, CodeBuild 공통).
fn write_properties_file(vars: &VersionVariables) {
    let lines: Vec<String> =
        vars.to_map().iter().map(|(k, v)| format!("GitVersion_{k}={v}")).collect();
    let _ = std::fs::write("gitversion.properties", lines.join("\n") + "\n");
}

struct GitLabCi;
impl BuildAgent for GitLabCi {
    fn name(&self) -> &'static str {
        "GitLabCi"
    }
    fn set_output_variable(&self, name: &str, value: &str) -> Vec<String> {
        key_value_line(name, value)
    }
    fn write_integration(&self, vars: &VersionVariables, ubn: bool) -> Vec<String> {
        let mut out = base_integration(self, vars, ubn);
        out.push("Outputting variables to 'gitversion.properties' ... ".into());
        write_properties_file(vars);
        out
    }
}

struct Jenkins;
impl BuildAgent for Jenkins {
    fn name(&self) -> &'static str {
        "Jenkins"
    }
    fn set_output_variable(&self, name: &str, value: &str) -> Vec<String> {
        key_value_line(name, value)
    }
    fn write_integration(&self, vars: &VersionVariables, ubn: bool) -> Vec<String> {
        let mut out = base_integration(self, vars, ubn);
        write_properties_file(vars);
        out.push("Outputting variables to 'gitversion.properties' ... ".into());
        out
    }
}

struct CodeBuild;
impl BuildAgent for CodeBuild {
    fn name(&self) -> &'static str {
        "CodeBuild"
    }
    fn set_output_variable(&self, name: &str, value: &str) -> Vec<String> {
        key_value_line(name, value)
    }
    fn write_integration(&self, vars: &VersionVariables, ubn: bool) -> Vec<String> {
        let out = base_integration(self, vars, ubn);
        write_properties_file(vars);
        out
    }
}

/// BitBucket Pipelines: 대문자 키, export 파일.
struct BitBucketPipelines;
impl BuildAgent for BitBucketPipelines {
    fn name(&self) -> &'static str {
        "BitBucketPipelines"
    }
    fn set_output_variable(&self, name: &str, value: &str) -> Vec<String> {
        vec![format!("GITVERSION_{}={value}", name.to_uppercase())]
    }
    fn write_integration(&self, vars: &VersionVariables, ubn: bool) -> Vec<String> {
        let mut out = base_integration(self, vars, ubn);
        let exports: Vec<String> = vars
            .to_map()
            .iter()
            .map(|(k, v)| format!("export GITVERSION_{}={v}", k.to_uppercase()))
            .collect();
        let _ = std::fs::write("gitversion.properties", exports.join("\n") + "\n");
        out.push("Outputting variables to 'gitversion.properties' ... ".into());
        out
    }
}

/// GitHub Actions: 변수는 $GITHUB_ENV 파일에 기록, stdout 에는 로그만.
struct GitHubActions;
impl BuildAgent for GitHubActions {
    fn name(&self) -> &'static str {
        "GitHubActions"
    }
    fn set_build_number(&self, _vars: &VersionVariables) -> String {
        String::new()
    }
    fn set_output_variable(&self, _name: &str, _value: &str) -> Vec<String> {
        Vec::new()
    }
    fn write_integration(&self, vars: &VersionVariables, ubn: bool) -> Vec<String> {
        let mut out = base_integration(self, vars, ubn);
        match env::var("GITHUB_ENV") {
            Ok(path) => {
                out.push(format!("Writing version variables to $GITHUB_ENV file for '{}'.", self.name()));
                let lines: Vec<String> = vars
                    .to_map()
                    .iter()
                    .filter(|(_, v)| !v.is_empty())
                    .map(|(k, v)| format!("GitVersion_{k}={v}"))
                    .collect();
                use std::io::Write;
                if let Ok(mut f) =
                    std::fs::OpenOptions::new().create(true).append(true).open(&path)
                {
                    let _ = writeln!(f, "{}", lines.join("\n"));
                }
            }
            Err(_) => out.push(
                "Unable to write GitVersion variables to $GITHUB_ENV because the environment variable is not set."
                    .into(),
            ),
        }
        out
    }
}

/// 출력 함수가 없는 CI(BuildKite, SpaceAutomation): 로그 라인만.
struct BuildKite;
impl BuildAgent for BuildKite {
    fn name(&self) -> &'static str {
        "BuildKite"
    }
    fn set_build_number(&self, _vars: &VersionVariables) -> String {
        String::new()
    }
    fn set_output_variable(&self, _name: &str, _value: &str) -> Vec<String> {
        Vec::new()
    }
}

struct SpaceAutomation;
impl BuildAgent for SpaceAutomation {
    fn name(&self) -> &'static str {
        "SpaceAutomation"
    }
    fn set_build_number(&self, _vars: &VersionVariables) -> String {
        String::new()
    }
    fn set_output_variable(&self, _name: &str, _value: &str) -> Vec<String> {
        Vec::new()
    }
}

/// AppVeyor: 실제로는 REST API 호출. 오프라인에서는 로그 라인만 출력.
struct AppVeyor;
impl BuildAgent for AppVeyor {
    fn name(&self) -> &'static str {
        "AppVeyor"
    }
    fn set_build_number(&self, vars: &VersionVariables) -> String {
        format!("Set AppVeyor build number to '{}'.", vars.full_sem_ver)
    }
    fn set_output_variable(&self, name: &str, value: &str) -> Vec<String> {
        vec![format!("Adding Environment Variable. name='GitVersion_{name}' value='{value}']")]
    }
}

/// 환경변수로 현재 빌드에이전트 감지. 원본 등록 순서와 유사.
pub fn detect() -> Option<Box<dyn BuildAgent>> {
    let has = |k: &str| env::var(k).map(|v| !v.is_empty()).unwrap_or(false);

    if has("TEAMCITY_VERSION") {
        Some(Box::new(TeamCity))
    } else if has("TF_BUILD") {
        Some(Box::new(AzurePipelines))
    } else if has("GITHUB_ACTIONS") {
        Some(Box::new(GitHubActions))
    } else if has("GITLAB_CI") {
        Some(Box::new(GitLabCi))
    } else if has("JENKINS_URL") {
        Some(Box::new(Jenkins))
    } else if has("CODEBUILD_WEBHOOK_HEAD_REF") {
        Some(Box::new(CodeBuild))
    } else if has("TRAVIS") {
        Some(Box::new(TravisCi))
    } else if has("DRONE") {
        Some(Box::new(Drone))
    } else if has("APPVEYOR") {
        Some(Box::new(AppVeyor))
    } else if has("ENVRUN_DATABASE") {
        Some(Box::new(EnvRun))
    } else if has("ContinuaCI.Version") {
        Some(Box::new(ContinuaCi))
    } else if has("BITBUCKET_WORKSPACE") {
        Some(Box::new(BitBucketPipelines))
    } else if has("BUILDKITE") {
        Some(Box::new(BuildKite))
    } else if has("JB_SPACE_PROJECT_KEY") {
        Some(Box::new(SpaceAutomation))
    } else if env::var("BuildRunner").map(|v| v.eq_ignore_ascii_case("MyGet")).unwrap_or(false) {
        Some(Box::new(MyGet))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> VersionVariables {
        VersionVariables { full_sem_ver: "1.0.1-1".into(), ..Default::default() }
    }

    #[test]
    fn teamcity_format() {
        let a = TeamCity;
        assert_eq!(a.set_build_number(&sample()), "##teamcity[buildNumber '1.0.1-1']");
        assert_eq!(
            a.set_output_variable("FullSemVer", "1.0.1-1"),
            vec![
                "##teamcity[setParameter name='GitVersion.FullSemVer' value='1.0.1-1']",
                "##teamcity[setParameter name='system.GitVersion.FullSemVer' value='1.0.1-1']",
            ]
        );
    }

    #[test]
    fn teamcity_escapes_special_chars() {
        let a = TeamCity;
        assert_eq!(
            a.set_output_variable("X", "a'b[c]"),
            vec![
                "##teamcity[setParameter name='GitVersion.X' value='a|'b|[c|]']",
                "##teamcity[setParameter name='system.GitVersion.X' value='a|'b|[c|]']",
            ]
        );
    }

    #[test]
    fn azure_format() {
        let a = AzurePipelines;
        assert_eq!(
            a.set_output_variable("Major", "1"),
            vec![
                "##vso[task.setvariable variable=GitVersion.Major]1",
                "##vso[task.setvariable variable=GitVersion.Major;isOutput=true]1",
            ]
        );
    }

    #[test]
    fn key_value_agents() {
        assert_eq!(GitLabCi.set_output_variable("Sha", "abc"), vec!["GitVersion_Sha=abc"]);
        assert_eq!(TravisCi.set_output_variable("Sha", "abc"), vec!["GitVersion_Sha=abc"]);
        assert_eq!(
            BitBucketPipelines.set_output_variable("FullSemVer", "1.0.1-1"),
            vec!["GITVERSION_FULLSEMVER=1.0.1-1"]
        );
    }

    #[test]
    fn integration_skips_build_number_when_disabled() {
        let out = TeamCity.write_integration(&sample(), false);
        assert!(out.iter().all(|l| !l.contains("buildNumber")));
        assert!(out.iter().any(|l| l.starts_with("Set Output Variables for 'TeamCity'.")));
    }

    #[test]
    fn integration_includes_build_number_when_enabled() {
        let out = TeamCity.write_integration(&sample(), true);
        assert!(out.iter().any(|l| l == "##teamcity[buildNumber '1.0.1-1']"));
    }
}
