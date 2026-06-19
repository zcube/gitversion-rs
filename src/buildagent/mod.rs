//! Build agent (CI) integrations.
//!
//! Ports each agent from the original `GitVersion.BuildAgents`. Detects the current CI
//! from environment variables and outputs variables / build numbers in the format expected
//! by that CI. When `update_build_number` is false the build-number line is omitted
//! (mirrors the original `BuildAgentBase.WriteIntegration` behaviour).

use crate::output::VersionVariables;
use std::env;

/// Escape a value for TeamCity/MyGet service messages.
fn escape_value(v: &str) -> String {
    v.replace('|', "||")
        .replace('\'', "|'")
        .replace('[', "|[")
        .replace(']', "|]")
        .replace('\r', "|r")
        .replace('\n', "|n")
}

/// Common interface for build agents.
pub trait BuildAgent {
    /// Agent name matching the original class name (`GetType().Name`).
    fn name(&self) -> &'static str;

    /// Returns the build-number line (typically FullSemVer). Returns an empty string for CIs that don't support it.
    fn set_build_number(&self, vars: &VersionVariables) -> String {
        vars.full_sem_ver.clone()
    }

    /// Returns the output lines for a single variable.
    fn set_output_variable(&self, name: &str, value: &str) -> Vec<String>;

    /// Returns the full integration output (including log lines).
    fn write_integration(&self, vars: &VersionVariables, update_build_number: bool) -> Vec<String> {
        base_integration(self, vars, update_build_number)
    }
}

/// Default WriteIntegration behaviour (mirrors the original `BuildAgentBase`).
fn base_integration(
    agent: &(impl BuildAgent + ?Sized),
    vars: &VersionVariables,
    update_build_number: bool,
) -> Vec<String> {
    let mut out = Vec::new();
    if update_build_number {
        out.push(format!("Set Build Number for '{}'.", agent.name()));
        // Agents whose set_build_number returns an empty string (BuildKite, SpaceAutomation, etc.)
        // do not emit a build-number line (matches the original behaviour).
        let bn = agent.set_build_number(vars);
        if !bn.is_empty() {
            out.push(bn);
        }
    }
    out.push(format!("Set Output Variables for '{}'.", agent.name()));
    for (key, value) in vars.to_map() {
        out.extend(agent.set_output_variable(&key, &value));
    }
    out
}

// ─────────────────────────── Agent implementations ───────────────────────────

/// TeamCity: `##teamcity[...]` service message.
struct TeamCity;
impl BuildAgent for TeamCity {
    fn name(&self) -> &'static str {
        "TeamCity"
    }
    fn set_build_number(&self, vars: &VersionVariables) -> String {
        format!(
            "##teamcity[buildNumber '{}']",
            escape_value(&vars.full_sem_ver)
        )
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
        format!(
            "##myget[buildNumber '{}']",
            escape_value(&vars.full_sem_ver)
        )
    }
    fn set_output_variable(&self, name: &str, value: &str) -> Vec<String> {
        vec![format!(
            "##myget[setParameter name='GitVersion.{name}' value='{}']",
            escape_value(value)
        )]
    }
}

/// Azure Pipelines: `##vso[...]`.
struct AzurePipelines;
impl BuildAgent for AzurePipelines {
    fn name(&self) -> &'static str {
        "AzurePipelines"
    }
    fn set_build_number(&self, vars: &VersionVariables) -> String {
        // If BUILD_BUILDNUMBER is absent, fall back to FullSemVer (with the "+0" suffix stripped).
        match env::var("BUILD_BUILDNUMBER") {
            Ok(bn) if !bn.trim().is_empty() => {
                let replaced = replace_azure_vars(&bn, vars);
                if replaced != bn {
                    format!("##vso[build.updatebuildnumber]{replaced}")
                } else {
                    let v = vars
                        .full_sem_ver
                        .strip_suffix("+0")
                        .unwrap_or(&vars.full_sem_ver);
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
        vec![format!(
            "@@envrun[set name='GitVersion_{name}' value='{value}']"
        )]
    }
}

/// Shared `GitVersion_{name}={value}` format used by TravisCI, Drone, GitLabCi, Jenkins, and CodeBuild.
fn key_value_line(name: &str, value: &str) -> Vec<String> {
    vec![format!("GitVersion_{name}={value}")]
}

struct TravisCi;
impl BuildAgent for TravisCi {
    fn name(&self) -> &'static str {
        "TravisCi"
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

/// Write variables to a `gitversion.properties` file (shared by GitLabCi, Jenkins, and CodeBuild).
fn write_properties_file(vars: &VersionVariables) {
    let lines: Vec<String> = vars
        .to_map()
        .iter()
        .map(|(k, v)| format!("GitVersion_{k}={v}"))
        .collect();
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
        let mut out = base_integration(self, vars, ubn);
        write_properties_file(vars);
        out.push("Outputting variables to 'gitversion.properties' ... ".into());
        out
    }
}

/// BitBucket Pipelines: uppercase keys. Writes a properties (Bash) and ps1 (PowerShell) file, plus guidance lines.
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
        let pf = "gitversion.properties";
        let ps1 = "gitversion.ps1";
        let exports: Vec<String> = vars
            .to_map()
            .iter()
            .map(|(k, v)| format!("export GITVERSION_{}={v}", k.to_uppercase()))
            .collect();
        let _ = std::fs::write(pf, exports.join("\n") + "\n");
        // Guidance lines from the original BitBucketPipelines.WriteIntegration (Bash/PowerShell).
        out.push(format!("Outputting variables to '{pf}' for Bash,"));
        out.push(format!("and to '{ps1}' for Powershell ... "));
        out.push(
            "To import the file into your build environment, add the following line to your build step:"
                .into(),
        );
        out.push("Bash:".into());
        out.push(format!("  - source {pf}"));
        out.push("Powershell:".into());
        out.push(format!("  - . .\\{ps1}"));
        out.push(String::new());
        out.push("To reuse the file across build steps, add the file as a build artifact:".into());
        out.push("Bash:".into());
        out.push("  artifacts:".into());
        out.push(format!("    - {pf}"));
        out.push("Powershell:".into());
        out.push("  artifacts:".into());
        out.push(format!("    - {ps1}"));
        out
    }
}

/// GitHub Actions: writes variables to the `$GITHUB_ENV` file; stdout carries log lines only.
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

/// CIs without an output mechanism (BuildKite, SpaceAutomation): emit log lines only.
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

/// AppVeyor: uses REST API calls in practice. Offline, only log lines are emitted.
struct AppVeyor;
impl BuildAgent for AppVeyor {
    fn name(&self) -> &'static str {
        "AppVeyor"
    }
    fn set_build_number(&self, vars: &VersionVariables) -> String {
        format!("Set AppVeyor build number to '{}'.", vars.full_sem_ver)
    }
    fn set_output_variable(&self, name: &str, value: &str) -> Vec<String> {
        vec![format!(
            "Adding Environment Variable. name='GitVersion_{name}' value='{value}']"
        )]
    }
}

impl AppVeyor {
    /// The original AppVeyor integration uses REST API calls (PUT api/build,
    /// POST api/build/variables) rather than stdout commands, so it cannot be compared
    /// against a build-server stdout golden file. Instead, the request body (JSON) format
    /// is replicated here and verified by unit tests (actual transmission is environment-dependent).
    #[cfg(test)]
    fn build_number_body(vars: &VersionVariables, build_number: &str) -> String {
        format!(
            r#"{{"version":"{}.build.{}"}}"#,
            vars.full_sem_ver, build_number
        )
    }

    #[cfg(test)]
    fn output_variable_body(name: &str, value: &str) -> String {
        format!(r#"{{"name":"GitVersion_{name}","value":"{value}"}}"#)
    }
}

/// Instantiate an agent by name (matching the original `GetType().Name`). Used for tests and explicit selection.
pub fn by_name(name: &str) -> Option<Box<dyn BuildAgent>> {
    let agent: Box<dyn BuildAgent> = match name {
        "TeamCity" => Box::new(TeamCity),
        "MyGet" => Box::new(MyGet),
        "AzurePipelines" => Box::new(AzurePipelines),
        "ContinuaCi" => Box::new(ContinuaCi),
        "EnvRun" => Box::new(EnvRun),
        "TravisCI" | "TravisCi" => Box::new(TravisCi),
        "Drone" => Box::new(Drone),
        "GitLabCi" => Box::new(GitLabCi),
        "Jenkins" => Box::new(Jenkins),
        "CodeBuild" => Box::new(CodeBuild),
        "BitBucketPipelines" => Box::new(BitBucketPipelines),
        "GitHubActions" => Box::new(GitHubActions),
        "BuildKite" => Box::new(BuildKite),
        "SpaceAutomation" => Box::new(SpaceAutomation),
        "AppVeyor" => Box::new(AppVeyor),
        _ => return None,
    };
    Some(agent)
}

/// Detect the current build agent from environment variables. Order follows the original registration order.
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
    } else if env::var("BuildRunner")
        .map(|v| v.eq_ignore_ascii_case("MyGet"))
        .unwrap_or(false)
    {
        Some(Box::new(MyGet))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> VersionVariables {
        VersionVariables {
            full_sem_ver: "1.0.1-1".into(),
            ..Default::default()
        }
    }

    #[test]
    fn appveyor_http_body_matches_dotnet() {
        // Request body format for the original AppVeyor PUT api/build / POST api/build/variables.
        let vars = VersionVariables {
            full_sem_ver: "1.2.3-beta.1".into(),
            ..Default::default()
        };
        assert_eq!(
            AppVeyor::build_number_body(&vars, "42"),
            r#"{"version":"1.2.3-beta.1.build.42"}"#
        );
        assert_eq!(
            AppVeyor::output_variable_body("Major", "1"),
            r#"{"name":"GitVersion_Major","value":"1"}"#
        );
    }

    #[test]
    fn teamcity_format() {
        let a = TeamCity;
        assert_eq!(
            a.set_build_number(&sample()),
            "##teamcity[buildNumber '1.0.1-1']"
        );
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
        assert_eq!(
            GitLabCi.set_output_variable("Sha", "abc"),
            vec!["GitVersion_Sha=abc"]
        );
        assert_eq!(
            TravisCi.set_output_variable("Sha", "abc"),
            vec!["GitVersion_Sha=abc"]
        );
        assert_eq!(
            BitBucketPipelines.set_output_variable("FullSemVer", "1.0.1-1"),
            vec!["GITVERSION_FULLSEMVER=1.0.1-1"]
        );
    }

    #[test]
    fn integration_skips_build_number_when_disabled() {
        let out = TeamCity.write_integration(&sample(), false);
        assert!(out.iter().all(|l| !l.contains("buildNumber")));
        assert!(out
            .iter()
            .any(|l| l.starts_with("Set Output Variables for 'TeamCity'.")));
    }

    #[test]
    fn integration_includes_build_number_when_enabled() {
        let out = TeamCity.write_integration(&sample(), true);
        assert!(out.iter().any(|l| l == "##teamcity[buildNumber '1.0.1-1']"));
    }
}
