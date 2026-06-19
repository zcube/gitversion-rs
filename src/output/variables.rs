//! The final computed GitVersion output variables.
//!
//! Maps 1:1 to the original `GitVersion.Output/Serializer/VersionVariablesJsonModel.cs`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// All output variables computed by GitVersion.
///
/// JSON output uses the same key names as the original (PascalCase).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct VersionVariables {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,

    pub pre_release_tag: String,
    pub pre_release_tag_with_dash: String,
    pub pre_release_label: String,
    pub pre_release_label_with_dash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pre_release_number: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weighted_pre_release_number: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_meta_data: Option<i64>,
    pub full_build_meta_data: String,

    pub major_minor_patch: String,
    pub sem_ver: String,
    pub full_sem_ver: String,

    pub assembly_sem_ver: String,
    pub assembly_sem_file_ver: String,
    pub informational_version: String,

    pub branch_name: String,
    pub escaped_branch_name: String,
    pub sha: String,
    pub short_sha: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_source_distance: Option<i64>,
    pub version_source_increment: String,
    pub version_source_sem_ver: String,
    pub version_source_sha: String,
    /// Deprecated: prefer `VersionSourceDistance`. Retained for compatibility.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commits_since_version_source: Option<i64>,

    pub commit_date: String,
    pub uncommitted_changes: i64,
}

impl VersionVariables {
    /// Variable name to string-value map. Used by `-showvariable` and environment-variable output.
    pub fn to_map(&self) -> BTreeMap<String, String> {
        let mut m = BTreeMap::new();
        let opt = |o: Option<i64>| o.map(|v| v.to_string()).unwrap_or_default();
        m.insert("Major".into(), self.major.to_string());
        m.insert("Minor".into(), self.minor.to_string());
        m.insert("Patch".into(), self.patch.to_string());
        m.insert("PreReleaseTag".into(), self.pre_release_tag.clone());
        m.insert(
            "PreReleaseTagWithDash".into(),
            self.pre_release_tag_with_dash.clone(),
        );
        m.insert("PreReleaseLabel".into(), self.pre_release_label.clone());
        m.insert(
            "PreReleaseLabelWithDash".into(),
            self.pre_release_label_with_dash.clone(),
        );
        m.insert("PreReleaseNumber".into(), opt(self.pre_release_number));
        m.insert(
            "WeightedPreReleaseNumber".into(),
            opt(self.weighted_pre_release_number),
        );
        m.insert("BuildMetaData".into(), opt(self.build_meta_data));
        m.insert(
            "FullBuildMetaData".into(),
            self.full_build_meta_data.clone(),
        );
        m.insert("MajorMinorPatch".into(), self.major_minor_patch.clone());
        m.insert("SemVer".into(), self.sem_ver.clone());
        m.insert("FullSemVer".into(), self.full_sem_ver.clone());
        m.insert("AssemblySemVer".into(), self.assembly_sem_ver.clone());
        m.insert(
            "AssemblySemFileVer".into(),
            self.assembly_sem_file_ver.clone(),
        );
        m.insert(
            "InformationalVersion".into(),
            self.informational_version.clone(),
        );
        m.insert("BranchName".into(), self.branch_name.clone());
        m.insert("EscapedBranchName".into(), self.escaped_branch_name.clone());
        m.insert("Sha".into(), self.sha.clone());
        m.insert("ShortSha".into(), self.short_sha.clone());
        m.insert(
            "VersionSourceDistance".into(),
            opt(self.version_source_distance),
        );
        m.insert(
            "VersionSourceIncrement".into(),
            self.version_source_increment.clone(),
        );
        m.insert(
            "VersionSourceSemVer".into(),
            self.version_source_sem_ver.clone(),
        );
        m.insert("VersionSourceSha".into(), self.version_source_sha.clone());
        m.insert(
            "CommitsSinceVersionSource".into(),
            opt(self.commits_since_version_source),
        );
        m.insert("CommitDate".into(), self.commit_date.clone());
        m.insert(
            "UncommittedChanges".into(),
            self.uncommitted_changes.to_string(),
        );
        m
    }
}
