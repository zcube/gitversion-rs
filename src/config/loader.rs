//! 설정 파일 탐색, YAML 로딩, 기본값 병합.
//!
//! 원본 `GitVersion.Configuration/ConfigurationFileLocator.cs`,
//! `ConfigurationProvider.cs` 대응.

use super::{defaults, model::*};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// 탐색 순서대로의 설정 파일명.
const CANDIDATES: [&str; 4] =
    ["GitVersion.yml", "GitVersion.yaml", ".GitVersion.yml", ".GitVersion.yaml"];

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
        .with_context(|| format!("설정 파일을 읽을 수 없습니다: {}", path.display()))?;
    let overrides: GitVersionConfiguration = serde_yaml::from_str(&text)
        .with_context(|| format!("설정 파일 YAML 파싱 실패: {}", path.display()))?;

    let mut base = defaults::for_workflow(overrides.workflow.as_deref());
    merge(&mut base, overrides);
    Ok(base)
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
        base.merge_message_formats.extend(over.merge_message_formats);
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
