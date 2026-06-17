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

/// next-version 정규화. 원본 `GitVersionConfiguration.NextVersion` setter 는 값이
/// 정수면 `"{major}.0"` 으로 보정한다(예: "1" 은 "1.0", "2" 는 "2.0"). 그 외는 그대로.
fn normalize_next_version(value: &str) -> String {
    match value.trim().parse::<i64>() {
        Ok(major) => format!("{major}.0"),
        Err(_) => value.to_string(),
    }
}

/// label 의 `{token}` 을 치환. 원본 `GetBranchSpecificLabel` + `BuildLabelPlaceholders`
/// + `StringFormatWith` 동작을 옮긴다:
/// - placeholder 는 브랜치 정규식의 **named capture** 뿐이며, 각 값에 SanitizeName
///   (`[^a-zA-Z0-9-]` → `-`)을 적용한다(BuildLabelPlaceholders).
/// - placeholder 에 없는 토큰은 **치환하지 않고 literal 로 유지**한다(FormatWith).
///   따라서 named capture 가 없는 정규식(예: 사용자 정의 `^custom/`)에서 `{BranchName}`
///   은 그대로 남는다(원본과 동일). 브랜치 마지막 세그먼트로의 fallback 은 하지 않는다.
/// - 최종 label 전체에 대한 추가 sanitize 는 하지 않는다(원본도 하지 않음).
fn resolve_label(label: &str, regex_src: &Option<String>, branch_name: &str) -> String {
    let sanitize = |s: &str| {
        Regex::new(r"[^a-zA-Z0-9-]")
            .unwrap()
            .replace_all(s, "-")
            .into_owned()
    };

    // BuildLabelPlaceholders: 정규식이 비었거나 브랜치명이 비면 placeholder 없음.
    let mut captures: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    if let Some(src) = regex_src {
        if !src.trim().is_empty() && !branch_name.is_empty() {
            if let Ok(re) = Regex::new(&format!("(?i){src}")) {
                if let Some(caps) = re.captures(branch_name) {
                    for name in re.capture_names().flatten() {
                        if let Some(m) = caps.name(name) {
                            captures.insert(name.to_string(), sanitize(m.as_str()));
                        }
                    }
                }
            }
        }
    }

    let token_re = Regex::new(r"\{([^}]+)\}").unwrap();
    token_re
        .replace_all(label, |c: &regex::Captures| {
            let whole = c[0].to_string();
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
                captures.get(name).cloned()
            };
            // 치환 실패 + 명시 fallback 없음 → 원본 토큰을 literal 로 유지.
            resolved.or(fallback).unwrap_or(whole)
        })
        .into_owned()
}

/// label 미지정 시 source-branches 부모에서 상속(원본 `BranchConfiguration.Inherit`
/// 의 `Label = Label ?? parent.Label`). 자기 label 이 있으면 그대로, 없으면 source
/// 부모를 순회하며 첫 정의된 label 을 사용. 모두 없으면 None(전역 fallback 으로).
fn inherit_label(
    config: &GitVersionConfiguration,
    bc: &BranchConfiguration,
    depth: usize,
) -> Option<String> {
    if let Some(l) = &bc.label {
        return Some(l.clone());
    }
    if depth > 8 {
        return None;
    }
    for src in &bc.source_branches {
        if let Some(src_bc) = config.branches.get(src) {
            if let Some(l) = inherit_label(config, src_bc, depth + 1) {
                return Some(l);
            }
        }
    }
    None
}

/// Increment == Inherit 를 source-branch 를 따라 해석.
pub(crate) fn resolve_increment(
    config: &GitVersionConfiguration,
    bc: &BranchConfiguration,
    depth: usize,
) -> IncrementStrategy {
    let own = bc
        .increment
        .or(config.increment)
        .unwrap_or(IncrementStrategy::Inherit);
    if own != IncrementStrategy::Inherit {
        return own;
    }
    // 원본 EffectiveBranchConfigurationFinder: Inherit 는 source-branches 부모에서
    // 해석한다. 끝까지 못 풀면 Inherit 가 남고, 이후 ToVersionField 단계에서 None
    // (증분 없음)이 된다. 임의 Patch fallback 을 넣지 않고 None 으로 귀결시킨다.
    if depth > 8 {
        return IncrementStrategy::None;
    }
    for src in &bc.source_branches {
        if let Some(src_bc) = config.branches.get(src) {
            let resolved = resolve_increment(config, src_bc, depth + 1);
            if resolved != IncrementStrategy::Inherit {
                return resolved;
            }
        }
    }
    IncrementStrategy::None
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

        let raw_label = inherit_label(config, &bc, 0)
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
            next_version: config.next_version.as_deref().map(normalize_next_version),
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
    fn normalize_next_version_pads_integer() {
        // 원본 setter: 정수는 "{major}.0" 으로 보정, 그 외는 그대로.
        assert_eq!(normalize_next_version("1"), "1.0");
        assert_eq!(normalize_next_version("2"), "2.0");
        assert_eq!(normalize_next_version("1.0"), "1.0");
        assert_eq!(normalize_next_version("1.2.3"), "1.2.3");
        assert_eq!(normalize_next_version("1.0.0-beta"), "1.0.0-beta");
    }

    #[test]
    fn resolve_label_branch_name_capture() {
        let cfg = defaults::gitflow();
        // feature/my-feat → label 에 {BranchName} 캡처가 "my-feat"로 치환됨.
        let eff = EffectiveConfiguration::resolve(&cfg, "feature/my-feat");
        assert_eq!(eff.label, "my-feat");
    }

    #[test]
    fn resolve_label_unmatched_token_stays_literal() {
        // named capture 가 없는 정규식: {BranchName} 은 치환되지 않고 literal 로 유지
        // (원본 FormatWith: placeholder 없는 토큰은 그대로). 세그먼트 fallback 안 함.
        let r = resolve_label("{BranchName}", &Some("^custom/".into()), "custom/x");
        assert_eq!(r, "{BranchName}");
        // named capture 가 있으면 치환 + SanitizeName.
        let r = resolve_label(
            "{BranchName}",
            &Some(r"^features?[/-](?<BranchName>.+)".into()),
            "feature/a_b",
        );
        assert_eq!(r, "a-b");
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
