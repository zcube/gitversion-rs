//! 설정 파일 탐색, YAML 로딩, 기본값 병합.
//!
//! 원본 `GitVersion.Configuration/ConfigurationFileLocator.cs`,
//! `ConfigurationProvider.cs` 대응.

use super::{defaults, model::*};
use anyhow::{Context, Result};
use rust_i18n::t;
use std::path::{Path, PathBuf};

/// 탐색 순서대로의 설정 파일명.
const CANDIDATES: [&str; 4] = [
    "GitVersion.yml",
    "GitVersion.yaml",
    ".GitVersion.yml",
    ".GitVersion.yaml",
];

/// `dir` 와 `repo_root` 에서 설정 파일을 탐색.
pub fn locate(dir: &Path, repo_root: Option<&Path>) -> Option<PathBuf> {
    let mut search_dirs = vec![dir.to_path_buf()];
    if let Some(root) = repo_root {
        if root != dir {
            search_dirs.push(root.to_path_buf());
        }
    }
    for d in search_dirs {
        for name in CANDIDATES {
            let p = d.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// 명시 경로 또는 탐색으로 설정을 로드하고 워크플로 기본값과 병합.
pub fn load(
    explicit_path: Option<&Path>,
    work_dir: &Path,
    repo_root: Option<&Path>,
) -> Result<GitVersionConfiguration> {
    let path = match explicit_path {
        Some(p) => Some(p.to_path_buf()),
        None => locate(work_dir, repo_root),
    };

    let Some(path) = path else {
        // 파일 없음 → GitFlow 기본값.
        return Ok(defaults::gitflow());
    };

    let text = std::fs::read_to_string(&path)
        .with_context(|| t!("config.read_failed", path = path.display()))?;
    let overrides: GitVersionConfiguration = serde_yaml::from_str(&text)
        .with_context(|| t!("config.yaml_parse_failed", path = path.display()))?;

    let mut base = defaults::for_workflow(overrides.workflow.as_deref());
    merge(&mut base, overrides);
    apply_source_branch_mappings(&mut base);
    validate(&base).with_context(|| t!("config.validate_failed", path = path.display()))?;
    Ok(base)
}

/// 설정 검증(원본 ConfigurationBuilderBase.ValidateConfiguration).
///
/// 각 브랜치는 `regex` 가 있어야 하고, `source-branches` 는 설정된 브랜치만 참조해야
/// 한다. 위반 시 에러(원본은 ConfigurationException 으로 중단).
pub fn validate(config: &GitVersionConfiguration) -> Result<()> {
    const HELP: &str = "\nSee https://gitversion.net/docs/reference/configuration for more info";
    for (name, bc) in &config.branches {
        if bc.regex.is_none() {
            anyhow::bail!(
                "Branch configuration '{name}' is missing required configuration 'regex'{HELP}"
            );
        }
        let missing: Vec<&str> = bc
            .source_branches
            .iter()
            .filter(|sb| !config.branches.contains_key(*sb))
            .map(|s| s.as_str())
            .collect();
        if !missing.is_empty() {
            anyhow::bail!(
                "Branch configuration '{name}' defines these 'source-branches' that are not configured: '[{}]'{HELP}",
                missing.join(",")
            );
        }
    }
    Ok(())
}

/// `is-source-branch-for` 역매핑: 브랜치 A 가 `is-source-branch-for: [X]` 를 가지면
/// 대상 X 의 `source-branches` 에 A 를 추가한다(원본 ApplySourceBranchesSourceBranch).
pub fn apply_source_branch_mappings(config: &mut GitVersionConfiguration) {
    let mappings: Vec<(String, Vec<String>)> = config
        .branches
        .iter()
        .filter(|(_, b)| !b.is_source_branch_for.is_empty())
        .map(|(k, b)| (k.clone(), b.is_source_branch_for.clone()))
        .collect();
    for (source, targets) in mappings {
        for target in targets {
            if let Some(tb) = config.branches.get_mut(&target) {
                if !tb.source_branches.contains(&source) {
                    tb.source_branches.push(source.clone());
                }
            }
        }
    }
}

/// override 설정을 base 위에 덮어쓴다(Some/비어있지 않은 값만).
pub fn merge(base: &mut GitVersionConfiguration, over: GitVersionConfiguration) {
    macro_rules! ov {
        ($field:ident) => {
            if over.$field.is_some() {
                base.$field = over.$field;
            }
        };
    }
    ov!(workflow);
    ov!(assembly_versioning_scheme);
    ov!(assembly_file_versioning_scheme);
    ov!(assembly_informational_format);
    ov!(assembly_versioning_format);
    ov!(assembly_file_versioning_format);
    ov!(tag_prefix);
    ov!(version_in_branch_pattern);
    ov!(next_version);
    ov!(major_version_bump_message);
    ov!(minor_version_bump_message);
    ov!(patch_version_bump_message);
    ov!(no_bump_message);
    ov!(tag_pre_release_weight);
    ov!(commit_date_format);
    ov!(semantic_version_format);
    ov!(update_build_number);
    ov!(commit_message_convention);
    ov!(increment);
    ov!(mode);
    ov!(label);
    ov!(regex);
    ov!(commit_message_incrementing);
    ov!(prevent_increment);
    ov!(track_merge_target);
    ov!(track_merge_message);
    ov!(tracks_release_branches);
    ov!(is_release_branch);
    ov!(is_main_branch);
    ov!(pre_release_weight);

    if !over.strategies.is_empty() {
        base.strategies = over.strategies;
    }
    if !over.source_branches.is_empty() {
        base.source_branches = over.source_branches;
    }
    if !over.is_source_branch_for.is_empty() {
        base.is_source_branch_for = over.is_source_branch_for;
    }
    if over.ignore.commits_before.is_some() || !over.ignore.sha.is_empty() {
        base.ignore = over.ignore;
    }
    if !over.merge_message_formats.is_empty() {
        base.merge_message_formats
            .extend(over.merge_message_formats);
    }
    if !over.exec.is_empty() {
        base.exec.extend(over.exec);
    }

    // 브랜치별 병합.
    for (key, ob) in over.branches {
        let entry = base.branches.entry(key).or_default();
        merge_branch(entry, ob);
    }
}

fn merge_branch(base: &mut BranchConfiguration, over: BranchConfiguration) {
    macro_rules! ov {
        ($field:ident) => {
            if over.$field.is_some() {
                base.$field = over.$field;
            }
        };
    }
    ov!(regex);
    ov!(label);
    ov!(increment);
    ov!(mode);
    ov!(commit_message_incrementing);
    ov!(prevent_increment);
    ov!(track_merge_target);
    ov!(track_merge_message);
    ov!(tracks_release_branches);
    ov!(is_release_branch);
    ov!(is_main_branch);
    ov!(pre_release_weight);
    if !over.source_branches.is_empty() {
        base.source_branches = over.source_branches;
    }
    if !over.is_source_branch_for.is_empty() {
        base.is_source_branch_for = over.is_source_branch_for;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_from(yaml: &str) -> GitVersionConfiguration {
        let over: GitVersionConfiguration = serde_yaml::from_str(yaml).unwrap();
        let mut base = defaults::for_workflow(over.workflow.as_deref());
        merge(&mut base, over);
        apply_source_branch_mappings(&mut base);
        base
    }

    #[test]
    fn validate_rejects_missing_regex() {
        let c = config_from("branches:\n  custom:\n    label: x\n");
        let err = validate(&c).unwrap_err().to_string();
        assert!(err.contains("'custom'") && err.contains("'regex'"), "{err}");
    }

    #[test]
    fn validate_rejects_unknown_source_branch() {
        let c =
            config_from("branches:\n  custom:\n    regex: '^c$'\n    source-branches: [nope]\n");
        let err = validate(&c).unwrap_err().to_string();
        assert!(
            err.contains("not configured") && err.contains("nope"),
            "{err}"
        );
    }

    #[test]
    fn validate_accepts_defaults_and_valid_custom() {
        assert!(validate(&defaults::gitflow()).is_ok());
        assert!(validate(&defaults::githubflow()).is_ok());
        let c =
            config_from("branches:\n  custom:\n    regex: '^c$'\n    source-branches: [main]\n");
        assert!(validate(&c).is_ok());
    }

    #[test]
    fn source_branch_reverse_mapping() {
        let c = config_from(
            "branches:\n  myfeat:\n    regex: '^myfeat$'\n    is-source-branch-for: [main]\n",
        );
        assert!(c.branches["main"]
            .source_branches
            .contains(&"myfeat".to_string()));
    }
}
