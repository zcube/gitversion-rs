//! Resolution of the final (effective) configuration applied to a specific branch.
//!
//! Simplified port of the inheritance/merge rules from the original
//! `GitVersion.Core/Configuration/EffectiveConfiguration.cs` and
//! `EffectiveBranchConfigurationFinder.cs`.

use super::model::*;
use regex::Regex;

/// Return the branch-config key and its configuration that match `branch_name`.
/// Concrete (non-`unknown`) branches take priority; falls back to `unknown` when nothing else matches.
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

/// Normalise next-version. The original `GitVersionConfiguration.NextVersion` setter
/// coerces plain integers to `"{major}.0"` (e.g. "1" becomes "1.0", "2" becomes "2.0"); all other values are kept as-is.
fn normalize_next_version(value: &str) -> String {
    match value.trim().parse::<i64>() {
        Ok(major) => format!("{major}.0"),
        Err(_) => value.to_string(),
    }
}

/// Substitute `{token}` placeholders in a label. Ports `GetBranchSpecificLabel` +
/// `BuildLabelPlaceholders` + `StringFormatWith` from the original:
/// - Placeholders come exclusively from **named captures** in the branch regex; each captured
///   value is passed through SanitizeName (`[^a-zA-Z0-9-]` → `-`) (BuildLabelPlaceholders).
/// - Tokens absent from the placeholder map are **left as-is** (FormatWith). Therefore
///   `{BranchName}` stays literal for a regex without named captures (e.g. a custom `^custom/`) —
///   matching the original. No fallback to the last branch-name segment is performed.
/// - No additional sanitisation is applied to the final label (the original does not do it either).
fn resolve_label(label: &str, regex_src: &Option<String>, branch_name: &str) -> String {
    let sanitize = |s: &str| {
        Regex::new(r"[^a-zA-Z0-9-]")
            .unwrap()
            .replace_all(s, "-")
            .into_owned()
    };

    // BuildLabelPlaceholders: no placeholders when the regex or branch name is empty.
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
            // Split `?? "fallback"`.
            let (expr, fallback) = match inner.split_once("??") {
                Some((l, r)) => (l.trim(), Some(r.trim().trim_matches('"').to_string())),
                None => (inner, None),
            };
            // Strip `:format` specifier (only the name is used).
            let name = expr.split(':').next().unwrap_or(expr).trim();
            let resolved = if let Some(var) = expr.strip_prefix("env:") {
                let var = var.split("??").next().unwrap_or(var).trim();
                std::env::var(var).ok().filter(|v| !v.is_empty())
            } else {
                captures.get(name).cloned()
            };
            // Resolution failure with no explicit fallback — keep the original token as a literal.
            resolved.or(fallback).unwrap_or(whole)
        })
        .into_owned()
}

/// Inherit label from source-branch parents when none is set locally (mirrors the original
/// `BranchConfiguration.Inherit` rule: `Label = Label ?? parent.Label`). Uses the branch's
/// own label when defined; otherwise walks source parents and returns the first defined label.
/// Returns None when no label is found (the caller then uses the global fallback).
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

/// Resolve an `Inherit` increment by walking the source-branch chain.
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
    // Per the original EffectiveBranchConfigurationFinder: Inherit is resolved by walking
    // source-branch parents. If still unresolved, it would remain Inherit and become None
    // (no increment) in the ToVersionField step. We resolve to None rather than adding an
    // arbitrary Patch fallback.
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

/// All configuration values flattened to those effective for a given branch.
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
    /// Regex for extracting a number from the pre-release label.
    pub label_number_pattern: String,
}

impl EffectiveConfiguration {
    /// Merge the global configuration with the matched branch configuration to produce the effective configuration.
    pub fn resolve(config: &GitVersionConfiguration, branch_name: &str) -> Self {
        let matched = find_branch_config(config, branch_name);
        let (branch_key, bc): (String, BranchConfiguration) = match matched {
            Some((k, b)) => (k, b.clone()),
            None => ("unknown".into(), BranchConfiguration::default()),
        };

        // null-coalescing: branch value takes priority, falls back to global.
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
        // "totally-unknown" should not match any pattern, so the "unknown" entry is returned.
        let result = find_branch_config(&cfg, "totally-unknown-xyz-branch");
        // If the "unknown" key exists, it is returned; otherwise None.
        if let Some((key, _)) = result {
            assert_eq!(key, "unknown");
        }
    }

    #[test]
    fn find_branch_config_short_name_matching() {
        let cfg = defaults::gitflow();
        // Long names such as "refs/heads/develop" should also match via the short form ("develop").
        let result = find_branch_config(&cfg, "refs/heads/develop");
        assert!(result.is_some());
        let (key, _) = result.unwrap();
        assert_eq!(key, "develop");
    }

    #[test]
    fn normalize_next_version_pads_integer() {
        // Original setter: integers are coerced to "{major}.0", everything else is kept as-is.
        assert_eq!(normalize_next_version("1"), "1.0");
        assert_eq!(normalize_next_version("2"), "2.0");
        assert_eq!(normalize_next_version("1.0"), "1.0");
        assert_eq!(normalize_next_version("1.2.3"), "1.2.3");
        assert_eq!(normalize_next_version("1.0.0-beta"), "1.0.0-beta");
    }

    #[test]
    fn resolve_label_branch_name_capture() {
        let cfg = defaults::gitflow();
        // feature/my-feat → the {BranchName} capture is substituted with "my-feat".
        let eff = EffectiveConfiguration::resolve(&cfg, "feature/my-feat");
        assert_eq!(eff.label, "my-feat");
    }

    #[test]
    fn resolve_label_unmatched_token_stays_literal() {
        // Regex without named captures: {BranchName} is not substituted and stays as a literal
        // (original FormatWith: tokens with no matching placeholder are kept). No segment fallback.
        let r = resolve_label("{BranchName}", &Some("^custom/".into()), "custom/x");
        assert_eq!(r, "{BranchName}");
        // With named captures, substitution + SanitizeName is applied.
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
        // feature/my.feature → BranchName = "my.feature" → label = "my-feature" ("." → "-").
        let eff = EffectiveConfiguration::resolve(&cfg, "feature/my.feature");
        assert_eq!(eff.label, "my-feature");
    }

    #[test]
    fn resolve_increment_inherit_falls_back_to_patch() {
        let cfg = defaults::gitflow();
        // develop is Inherit, so it inherits Patch from its source branch (main).
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
