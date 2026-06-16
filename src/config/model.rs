//! GitVersion 설정 데이터 모델.
//!
//! 원본 `schemas/6.3/GitVersion.configuration.json` 및
//! `GitVersion.Configuration/GitVersionConfiguration.cs` 와 1:1 대응한다.
//! YAML 키는 모두 kebab-case.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 증분 전략. `increment` 키.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IncrementStrategy {
    None,
    Major,
    Minor,
    Patch,
    Inherit,
}

/// 배포 모드. `mode` 키.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeploymentMode {
    ManualDeployment,
    ContinuousDelivery,
    ContinuousDeployment,
}

/// 커밋 메시지 기반 증분 동작. `commit-message-incrementing` 키.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommitMessageIncrementMode {
    Enabled,
    Disabled,
    MergeMessageOnly,
}

/// 버전 탐색 전략. `strategies` 키.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VersionStrategy {
    None,
    Fallback,
    ConfiguredNextVersion,
    MergeMessage,
    TaggedCommit,
    TrackReleaseBranches,
    VersionInBranchName,
    Mainline,
}

/// AssemblyVersion / AssemblyFileVersion 부여 스킴.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VersioningScheme {
    MajorMinorPatchTag,
    MajorMinorPatch,
    MajorMinor,
    Major,
    None,
}

/// SemanticVersion 파싱 엄격도.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SemanticVersionFormat {
    Strict,
    Loose,
}

/// increment 방지 설정. `prevent-increment` 키.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct PreventIncrement {
    #[serde(default)]
    pub of_merged_branch: Option<bool>,
    #[serde(default)]
    pub when_branch_merged: Option<bool>,
    #[serde(default)]
    pub when_current_commit_tagged: Option<bool>,
}

/// 무시할 커밋 설정. `ignore` 키.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct IgnoreConfig {
    #[serde(default)]
    pub commits_before: Option<String>,
    #[serde(default)]
    pub sha: Vec<String>,
    /// 이 경로 아래 파일만 변경한 커밋을 버전 계산에서 제외.
    /// 커밋의 변경 파일 전부가 무시 경로에 속할 때만 제외된다.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
}

/// 개별 브랜치 설정. 전역 설정에서 상속받아 병합된다.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct BranchConfiguration {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub regex: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub increment: Option<IncrementStrategy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<DeploymentMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_message_incrementing: Option<CommitMessageIncrementMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prevent_increment: Option<PreventIncrement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track_merge_target: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track_merge_message: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tracks_release_branches: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_release_branch: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_main_branch: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre_release_weight: Option<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_branches: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub is_source_branch_for: Vec<String>,
}

/// 루트 GitVersion 설정.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct GitVersionConfiguration {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assembly_versioning_scheme: Option<VersioningScheme>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assembly_file_versioning_scheme: Option<VersioningScheme>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assembly_informational_format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assembly_versioning_format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assembly_file_versioning_format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag_prefix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_in_branch_pattern: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub major_version_bump_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minor_version_bump_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patch_version_bump_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_bump_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag_pre_release_weight: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_date_format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_version_format: Option<SemanticVersionFormat>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update_build_number: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub strategies: Vec<VersionStrategy>,

    // 브랜치 단위로도 지정 가능한 전역 기본값
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub increment: Option<IncrementStrategy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<DeploymentMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub regex: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_message_incrementing: Option<CommitMessageIncrementMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prevent_increment: Option<PreventIncrement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track_merge_target: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track_merge_message: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tracks_release_branches: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_release_branch: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_main_branch: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre_release_weight: Option<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_branches: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub is_source_branch_for: Vec<String>,

    #[serde(default)]
    pub ignore: IgnoreConfig,
    #[serde(default)]
    pub merge_message_formats: BTreeMap<String, String>,
    #[serde(default)]
    pub branches: BTreeMap<String, BranchConfiguration>,

    /// 외부 명령 훅(semantic-release exec 유사). 훅 이름 -> 쉘 명령.
    /// 지원 훅: verify, prepare, publish, success, fail.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub exec: BTreeMap<String, String>,
}
