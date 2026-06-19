//! GitVersion configuration data model.
//!
//! Maps 1:1 to the original `schemas/6.3/GitVersion.configuration.json` and
//! `GitVersion.Configuration/GitVersionConfiguration.cs`.
//! All YAML keys are kebab-case.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Increment strategy. `increment` key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IncrementStrategy {
    None,
    Major,
    Minor,
    Patch,
    Inherit,
}

/// Deployment mode. `mode` key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeploymentMode {
    ManualDeployment,
    ContinuousDelivery,
    ContinuousDeployment,
}

/// Commit-message-based increment behaviour. `commit-message-incrementing` key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommitMessageIncrementMode {
    Enabled,
    Disabled,
    MergeMessageOnly,
}

/// Version discovery strategy. `strategies` key.
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

/// Versioning scheme for AssemblyVersion / AssemblyFileVersion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VersioningScheme {
    MajorMinorPatchTag,
    MajorMinorPatch,
    MajorMinor,
    Major,
    None,
}

/// SemanticVersion parsing strictness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SemanticVersionFormat {
    Strict,
    Loose,
}

/// Prevent-increment configuration. `prevent-increment` key.
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

/// Commit-ignore configuration. `ignore` key.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct IgnoreConfig {
    #[serde(default)]
    pub commits_before: Option<String>,
    #[serde(default)]
    pub sha: Vec<String>,
    /// Exclude commits that only touch files under these paths from version calculation.
    /// A commit is excluded only when *all* its changed files fall under the ignored paths.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
}

/// Per-branch configuration. Merged with the global configuration via inheritance.
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
    /// Regex for extracting a number from the pre-release label.
    /// Corresponds to the original `BranchConfiguration.LabelNumberPattern`.
    /// When None, the built-in fixed pattern is used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_number_pattern: Option<String>,
}

/// Root GitVersion configuration.
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

    // Global defaults that can also be overridden per-branch
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
    /// Global default `label-number-pattern`. Can be overridden per branch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_number_pattern: Option<String>,

    #[serde(default)]
    pub ignore: IgnoreConfig,
    #[serde(default)]
    pub merge_message_formats: BTreeMap<String, String>,
    #[serde(default)]
    pub branches: BTreeMap<String, BranchConfiguration>,

    /// External command hooks (similar to the semantic-release exec plugin). Hook name -> shell command.
    /// Supported hooks: verify, prepare, publish, success, fail.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub exec: BTreeMap<String, String>,
}
