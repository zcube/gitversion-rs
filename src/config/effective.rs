//! 특정 브랜치에 적용될 최종(effective) 설정 해석.
//!
//! 원본 `GitVersion.Core/Configuration/EffectiveConfiguration.cs` 와
//! `EffectiveBranchConfigurationFinder.cs` 의 상속/병합 규칙을 단순화해 구현.

use super::model::*;
use regex::Regex;

/// 브랜치명에 매칭되는 브랜치 설정 키와 그 설정을 반환.
/// 구체적인 브랜치를 우선하고, 매칭이 없으면 `unknown` 을 사용한다.
pub fn find_branch_config<'a>(
    config: &'a GitVersionConfiguration,
    branch_name: &str,
) -> Option<(String, &'a BranchConfiguration)> {
    let short = branch_name.rsplit('/').next().unwrap_or(branch_name);
    let mut unknown: Option<(String, &BranchConfiguration)> = None;
    for (key, bc) in &config.branches {
        let Some(re_src) = &bc.regex else { continue };
        if re_src.is_empty() {
            continue;
        }
        let Ok(re) = Regex::new(&format!("(?i){re_src}")) else {
            continue;
        };
        if re.is_match(branch_name) || re.is_match(short) {
            if key == "unknown" {
                unknown = Some((key.clone(), bc));
            } else {
                return Some((key.clone(), bc));
            }
        }
    }
    unknown
}

/// 브랜치 정규식의 named capture, `{env:VAR}`, `?? fallback` 으로 label 을 치환.
/// 원본 Formatting/StringFormatWithExtension.cs 의 동작을 단순화해 옮긴다.
fn resolve_label(label: &str, regex_src: &Option<String>, branch_name: &str) -> String {
    // 브랜치 정규식의 named capture 수집.
    let mut captures: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    if let Some(src) = regex_src {
        if let Ok(re) = Regex::new(&format!("(?i){src}")) {
            if let Some(caps) = re.captures(branch_name) {
                for name in re.capture_names().flatten() {
                    if let Some(m) = caps.name(name) {
                        captures.insert(name.to_string(), m.as_str().to_string());
                    }
                }
            }
        }
    }
    // 미지정 BranchName 은 브랜치 마지막 세그먼트로.
    captures.entry("BranchName".into()).or_insert_with(|| {
        branch_name
            .rsplit('/')
            .next()
            .unwrap_or(branch_name)
            .to_string()
    });

    let token_re = Regex::new(r"\{([^}]+)\}").unwrap();
    let out = token_re
        .replace_all(label, |c: &regex::Captures| {
            let inner = c[1].trim();
            // `?? "fallback"` 분리.
            let (expr, fallback) = match inner.split_once("??") {
                Some((l, r)) => (l.trim(), Some(r.trim().trim_matches('"').to_string())),
                None => (inner, None),
            };
            // `:format` 지정자 분리(이름만 사용).
            let name = expr.split(':').next().unwrap_or(expr).trim();
            let resolved = if let Some(var) = expr.strip_prefix("env:") {
                let var = var.split("??").next().unwrap_or(var).trim();
                std::env::var(var).ok().filter(|v| !v.is_empty())
            } else {
                captures.get(name).cloned().filter(|v| !v.is_empty())
            };
            resolved.or(fallback).unwrap_or_default()
        })
        .into_owned();

    // 원본 SanitizeName 규칙("[^a-zA-Z0-9-]"): 영숫자·하이픈이 아닌 문자를 모두
    // '-'로 교체한다(예: "(no branch)" → "-no-branch-", "a/b.c_d" → "a-b-c-d").
    Regex::new(r"[^a-zA-Z0-9-]")
        .unwrap()
        .replace_all(&out, "-")
        .into_owned()
}

/// Increment == Inherit 를 source-branch 를 따라 해석.
fn resolve_increment(
    config: &GitVersionConfiguration,
    bc: &BranchConfiguration,
    depth: usize,
) -> IncrementStrategy {
    let own = bc
        .increment
        .or(config.increment)
        .unwrap_or(IncrementStrategy::Inherit);
    if own != IncrementStrategy::Inherit || depth > 8 {
        return if own == IncrementStrategy::Inherit {
            IncrementStrategy::Patch
        } else {
            own
        };
    }
    for src in &bc.source_branches {
        if let Some(src_bc) = config.branches.get(src) {
            let resolved = resolve_increment(config, src_bc, depth + 1);
            if resolved != IncrementStrategy::Inherit {
                return resolved;
            }
        }
    }
    IncrementStrategy::Patch
}

/// 브랜치에 적용되는 모든 설정값을 평탄화한 구조.
#[derive(Debug, Clone)]
pub struct EffectiveConfiguration {
    pub branch_key: String,
    pub deployment_mode: DeploymentMode,
    pub label: String,
    pub increment: IncrementStrategy,
    pub regex: Option<String>,
    pub prevent_increment_of_merged_branch: bool,
    pub prevent_increment_when_branch_merged: bool,
    pub prevent_increment_when_current_commit_tagged: bool,
    pub track_merge_target: bool,
    pub track_merge_message: bool,
    pub tracks_release_branches: bool,
    pub is_release_branch: bool,
    pub is_main_branch: bool,
    pub pre_release_weight: i64,
    pub tag_pre_release_weight: i64,
    pub commit_message_incrementing: CommitMessageIncrementMode,
    pub major_bump_message: String,
    pub minor_bump_message: String,
    pub patch_bump_message: String,
    pub no_bump_message: String,
    pub tag_prefix: String,
    pub version_in_branch_pattern: String,
    pub next_version: Option<String>,
    pub semantic_version_format: SemanticVersionFormat,
    pub commit_date_format: String,
    pub assembly_versioning_scheme: VersioningScheme,
    pub assembly_file_versioning_scheme: VersioningScheme,
    pub assembly_informational_format: String,
    pub assembly_versioning_format: Option<String>,
    pub assembly_file_versioning_format: Option<String>,
    pub merge_message_formats: std::collections::BTreeMap<String, String>,
    pub source_branches: Vec<String>,
    /// pre-release label 에서 번호를 추출하는 정규식.
    pub label_number_pattern: String,
}

impl EffectiveConfiguration {
    /// 전역 설정 + 매칭된 브랜치 설정을 병합해 effective 설정 생성.
    pub fn resolve(config: &GitVersionConfiguration, branch_name: &str) -> Self {
        let matched = find_branch_config(config, branch_name);
        let (branch_key, bc): (String, BranchConfiguration) = match matched {
            Some((k, b)) => (k, b.clone()),
            None => ("unknown".into(), BranchConfiguration::default()),
        };

        // null-coalescing: branch 값 우선, 없으면 global.
        let pi_branch = bc.prevent_increment.clone().unwrap_or_default();
        let pi_global = config.prevent_increment.clone().unwrap_or_default();
        let coalesce_bool = |b: Option<bool>, g: Option<bool>| b.or(g).unwrap_or(false);

        let raw_label = bc
            .label
            .clone()
            .or_else(|| config.label.clone())
            .unwrap_or_default();
        let label = resolve_label(&raw_label, &bc.regex, branch_name);

        EffectiveConfiguration {
            deployment_mode: bc
                .mode
                .or(config.mode)
                .unwrap_or(DeploymentMode::ContinuousDelivery),
            label,
            increment: resolve_increment(config, &bc, 0),
            regex: bc.regex.clone(),
            prevent_increment_of_merged_branch: coalesce_bool(
                pi_branch.of_merged_branch,
                pi_global.of_merged_branch,
            ),
            prevent_increment_when_branch_merged: coalesce_bool(
                pi_branch.when_branch_merged,
                pi_global.when_branch_merged,
            ),
            prevent_increment_when_current_commit_tagged: pi_branch
                .when_current_commit_tagged
                .or(pi_global.when_current_commit_tagged)
                .unwrap_or(true),
            track_merge_target: coalesce_bool(bc.track_merge_target, config.track_merge_target),
            track_merge_message: bc
                .track_merge_message
                .or(config.track_merge_message)
                .unwrap_or(true),
            tracks_release_branches: coalesce_bool(
                bc.tracks_release_branches,
                config.tracks_release_branches,
            ),
            is_release_branch: coalesce_bool(bc.is_release_branch, config.is_release_branch),
            is_main_branch: coalesce_bool(bc.is_main_branch, config.is_main_branch),
            pre_release_weight: bc
                .pre_release_weight
                .or(config.pre_release_weight)
                .unwrap_or(0),
            tag_pre_release_weight: config.tag_pre_release_weight.unwrap_or(60000),
            commit_message_incrementing: bc
                .commit_message_incrementing
                .or(config.commit_message_incrementing)
                .unwrap_or(CommitMessageIncrementMode::Enabled),
            major_bump_message: config
                .major_version_bump_message
                .clone()
                .unwrap_or_else(|| r"\+semver:\s?(breaking|major)".into()),
            minor_bump_message: config
                .minor_version_bump_message
                .clone()
                .unwrap_or_else(|| r"\+semver:\s?(feature|minor)".into()),
            patch_bump_message: config
                .patch_version_bump_message
                .clone()
                .unwrap_or_else(|| r"\+semver:\s?(fix|patch)".into()),
            no_bump_message: config
                .no_bump_message
                .clone()
                .unwrap_or_else(|| r"\+semver:\s?(none|skip)".into()),
            tag_prefix: config.tag_prefix.clone().unwrap_or_else(|| "[vV]?".into()),
            version_in_branch_pattern: config
                .version_in_branch_pattern
                .clone()
                .unwrap_or_else(|| r"(?<version>[vV]?\d+(\.\d+)?(\.\d+)?).*".into()),
            next_version: config.next_version.clone(),
            semantic_version_format: config
                .semantic_version_format
                .unwrap_or(SemanticVersionFormat::Strict),
            commit_date_format: config
                .commit_date_format
                .clone()
                .unwrap_or_else(|| "yyyy-MM-dd".into()),
            assembly_versioning_scheme: config
                .assembly_versioning_scheme
                .unwrap_or(VersioningScheme::MajorMinorPatch),
            assembly_file_versioning_scheme: config
                .assembly_file_versioning_scheme
                .unwrap_or(VersioningScheme::MajorMinorPatch),
            assembly_informational_format: config
                .assembly_informational_format
                .clone()
                .unwrap_or_else(|| "{InformationalVersion}".into()),
            assembly_versioning_format: config.assembly_versioning_format.clone(),
            assembly_file_versioning_format: config.assembly_file_versioning_format.clone(),
            merge_message_formats: config.merge_message_formats.clone(),
            source_branches: bc.source_branches.clone(),
            label_number_pattern: bc
                .label_number_pattern
                .clone()
                .or_else(|| config.label_number_pattern.clone())
                .unwrap_or_else(|| r"(?<name>.*?)\.?(?<number>\d+)?$".into()),
            branch_key,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::defaults;

    #[test]
    fn find_branch_config_main_matches() {
        let cfg = defaults::gitflow();
        let (key, _) = find_branch_config(&cfg, "main").unwrap();
        assert_eq!(key, "main");
    }

    #[test]
    fn find_branch_config_feature_matches() {
        let cfg = defaults::gitflow();
        let (key, _) = find_branch_config(&cfg, "feature/foo").unwrap();
        assert_eq!(key, "feature");
    }

    #[test]
    fn find_branch_config_no_match_returns_unknown() {
        let cfg = defaults::gitflow();
        // "totally-unknown"은 어느 패턴에도 매칭되지 않아야 함 → unknown 반환.
        let result = find_branch_config(&cfg, "totally-unknown-xyz-branch");
        // unknown 키가 있으면 그것을 반환, 없으면 None.
        if let Some((key, _)) = result {
            assert_eq!(key, "unknown");
        }
    }

    #[test]
    fn find_branch_config_short_name_matching() {
        let cfg = defaults::gitflow();
        // "refs/heads/develop" 처럼 긴 이름도 short("develop")로 매칭됨.
        let result = find_branch_config(&cfg, "refs/heads/develop");
        assert!(result.is_some());
        let (key, _) = result.unwrap();
        assert_eq!(key, "develop");
    }

    #[test]
    fn resolve_label_branch_name_capture() {
        let cfg = defaults::gitflow();
        // feature/my-feat → label 에 {BranchName} 캡처가 "my-feat"로 치환됨.
        let eff = EffectiveConfiguration::resolve(&cfg, "feature/my-feat");
        assert_eq!(eff.label, "my-feat");
    }

    #[test]
    fn resolve_label_slash_dot_sanitized() {
        let cfg = defaults::gitflow();
        // feature/my.feature → BranchName = "my.feature" → label = "my-feature"(. → -).
        let eff = EffectiveConfiguration::resolve(&cfg, "feature/my.feature");
        assert_eq!(eff.label, "my-feature");
    }

    #[test]
    fn resolve_increment_inherit_falls_back_to_patch() {
        let cfg = defaults::gitflow();
        // develop 은 Inherit 이므로 source 브랜치(main)의 Patch 를 상속.
        let eff = EffectiveConfiguration::resolve(&cfg, "develop");
        assert_eq!(eff.increment, crate::config::IncrementStrategy::Minor);
    }

    #[test]
    fn resolve_sets_is_main_branch_for_main() {
        let cfg = defaults::gitflow();
        let eff = EffectiveConfiguration::resolve(&cfg, "main");
        assert!(eff.is_main_branch);
    }

    #[test]
    fn resolve_sets_is_release_branch_for_release() {
        let cfg = defaults::gitflow();
        let eff = EffectiveConfiguration::resolve(&cfg, "release/1.0.0");
        assert!(eff.is_release_branch);
    }

    #[test]
    fn resolve_hotfix_inherits_patch() {
        let cfg = defaults::gitflow();
        let eff = EffectiveConfiguration::resolve(&cfg, "hotfix/1.0.1");
        assert_eq!(eff.increment, crate::config::IncrementStrategy::Patch);
    }
}
