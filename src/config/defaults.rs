//! Built-in default configuration per workflow.
//!
//! Ports the values from the original
//! `GitVersion.Configuration/Builders/{GitFlow,GitHubFlow,TrunkBased}ConfigurationBuilder.cs`.

use super::model::*;
use std::collections::BTreeMap;

const MAIN_REGEX: &str = "^master$|^main$";
const DEVELOP_REGEX: &str = "^dev(elop)?(ment)?$";
const RELEASE_REGEX: &str = r"^releases?[\/-](?<BranchName>.+)";
const FEATURE_REGEX: &str = r"^features?[\/-](?<BranchName>.+)";
const PR_REGEX: &str = r"^(pull-requests|pull|pr)[\/-](?<Number>\d*)";
const HOTFIX_REGEX: &str = r"^hotfix(es)?[\/-](?<BranchName>.+)";
const SUPPORT_REGEX: &str = r"^support[\/-](?<BranchName>.+)";
const UNKNOWN_REGEX: &str = "(?<BranchName>.+)";

const MAJOR_BUMP: &str = r"\+semver:\s?(breaking|major)";
const MINOR_BUMP: &str = r"\+semver:\s?(feature|minor)";
const PATCH_BUMP: &str = r"\+semver:\s?(fix|patch)";
const NO_BUMP: &str = r"\+semver:\s?(none|skip)";

fn prevent(
    of_merged: Option<bool>,
    when_merged: Option<bool>,
    when_tagged: Option<bool>,
) -> PreventIncrement {
    PreventIncrement {
        of_merged_branch: of_merged,
        when_branch_merged: when_merged,
        when_current_commit_tagged: when_tagged,
    }
}

/// Root configuration with global default fields populated (no branch entries).
fn global_base(mode: DeploymentMode, strategies: Vec<VersionStrategy>) -> GitVersionConfiguration {
    GitVersionConfiguration {
        assembly_versioning_scheme: Some(VersioningScheme::MajorMinorPatch),
        assembly_file_versioning_scheme: Some(VersioningScheme::MajorMinorPatch),
        assembly_informational_format: Some("{InformationalVersion}".into()),
        tag_prefix: Some("[vV]?".into()),
        version_in_branch_pattern: Some(r"(?<version>[vV]?\d+(\.\d+)?(\.\d+)?).*".into()),
        major_version_bump_message: Some(MAJOR_BUMP.into()),
        minor_version_bump_message: Some(MINOR_BUMP.into()),
        patch_version_bump_message: Some(PATCH_BUMP.into()),
        no_bump_message: Some(NO_BUMP.into()),
        tag_pre_release_weight: Some(60000),
        commit_date_format: Some("yyyy-MM-dd".into()),
        semantic_version_format: Some(SemanticVersionFormat::Strict),
        update_build_number: Some(true),
        strategies,
        increment: Some(IncrementStrategy::Inherit),
        mode: Some(mode),
        label: Some("{BranchName}".into()),
        regex: Some(String::new()),
        commit_message_incrementing: Some(CommitMessageIncrementMode::Enabled),
        prevent_increment: Some(prevent(Some(false), Some(false), Some(true))),
        track_merge_target: Some(false),
        track_merge_message: Some(true),
        tracks_release_branches: Some(false),
        is_release_branch: Some(false),
        is_main_branch: Some(false),
        ..Default::default()
    }
}

fn branch(regex: &str) -> BranchConfiguration {
    BranchConfiguration {
        regex: Some(regex.into()),
        ..Default::default()
    }
}

/// Default version strategies shared by GitFlow and GitHubFlow.
fn default_strategies() -> Vec<VersionStrategy> {
    vec![
        VersionStrategy::Fallback,
        VersionStrategy::ConfiguredNextVersion,
        VersionStrategy::MergeMessage,
        VersionStrategy::TaggedCommit,
        VersionStrategy::TrackReleaseBranches,
        VersionStrategy::VersionInBranchName,
    ]
}

/// GitFlow workflow (the default).
pub fn gitflow() -> GitVersionConfiguration {
    let mut c = global_base(DeploymentMode::ContinuousDelivery, default_strategies());
    let mut b: BTreeMap<String, BranchConfiguration> = BTreeMap::new();

    b.insert(
        "develop".into(),
        BranchConfiguration {
            increment: Some(IncrementStrategy::Minor),
            mode: Some(DeploymentMode::ContinuousDelivery),
            label: Some("alpha".into()),
            source_branches: vec!["main".into()],
            prevent_increment: Some(prevent(None, None, Some(false))),
            track_merge_target: Some(true),
            track_merge_message: Some(true),
            tracks_release_branches: Some(true),
            is_main_branch: Some(false),
            is_release_branch: Some(false),
            pre_release_weight: Some(0),
            ..branch(DEVELOP_REGEX)
        },
    );
    b.insert(
        "main".into(),
        BranchConfiguration {
            increment: Some(IncrementStrategy::Patch),
            label: Some(String::new()),
            source_branches: vec![],
            prevent_increment: Some(prevent(Some(true), None, None)),
            track_merge_target: Some(false),
            track_merge_message: Some(true),
            is_main_branch: Some(true),
            pre_release_weight: Some(55000),
            ..branch(MAIN_REGEX)
        },
    );
    b.insert(
        "release".into(),
        BranchConfiguration {
            increment: Some(IncrementStrategy::Minor),
            mode: Some(DeploymentMode::ManualDeployment),
            label: Some("beta".into()),
            source_branches: vec!["main".into(), "support".into()],
            prevent_increment: Some(prevent(Some(true), None, Some(false))),
            track_merge_target: Some(false),
            is_release_branch: Some(true),
            pre_release_weight: Some(30000),
            ..branch(RELEASE_REGEX)
        },
    );
    b.insert(
        "feature".into(),
        BranchConfiguration {
            increment: Some(IncrementStrategy::Inherit),
            mode: Some(DeploymentMode::ManualDeployment),
            label: Some("{BranchName}".into()),
            source_branches: vec![
                "develop".into(),
                "main".into(),
                "release".into(),
                "support".into(),
                "hotfix".into(),
            ],
            prevent_increment: Some(prevent(None, None, Some(false))),
            track_merge_message: Some(true),
            pre_release_weight: Some(30000),
            ..branch(FEATURE_REGEX)
        },
    );
    b.insert(
        "pull-request".into(),
        BranchConfiguration {
            increment: Some(IncrementStrategy::Inherit),
            mode: Some(DeploymentMode::ContinuousDelivery),
            label: Some("PullRequest{Number}".into()),
            source_branches: vec![
                "develop".into(),
                "main".into(),
                "release".into(),
                "feature".into(),
                "support".into(),
                "hotfix".into(),
            ],
            prevent_increment: Some(prevent(Some(true), None, Some(false))),
            track_merge_message: Some(true),
            pre_release_weight: Some(30000),
            ..branch(PR_REGEX)
        },
    );
    b.insert(
        "hotfix".into(),
        BranchConfiguration {
            increment: Some(IncrementStrategy::Inherit),
            mode: Some(DeploymentMode::ManualDeployment),
            label: Some("beta".into()),
            source_branches: vec!["main".into(), "support".into()],
            prevent_increment: Some(prevent(None, None, Some(false))),
            is_release_branch: Some(true),
            pre_release_weight: Some(30000),
            ..branch(HOTFIX_REGEX)
        },
    );
    b.insert(
        "support".into(),
        BranchConfiguration {
            increment: Some(IncrementStrategy::Patch),
            label: Some(String::new()),
            source_branches: vec!["main".into()],
            prevent_increment: Some(prevent(Some(true), None, None)),
            track_merge_target: Some(false),
            is_main_branch: Some(true),
            pre_release_weight: Some(55000),
            ..branch(SUPPORT_REGEX)
        },
    );
    b.insert(
        "unknown".into(),
        BranchConfiguration {
            increment: Some(IncrementStrategy::Inherit),
            mode: Some(DeploymentMode::ManualDeployment),
            label: Some("{BranchName}".into()),
            source_branches: vec![
                "main".into(),
                "develop".into(),
                "release".into(),
                "feature".into(),
                "pull-request".into(),
                "support".into(),
                "hotfix".into(),
            ],
            ..branch(UNKNOWN_REGEX)
        },
    );

    c.branches = b;
    c
}

/// GitHubFlow workflow.
pub fn githubflow() -> GitVersionConfiguration {
    let mut c = global_base(DeploymentMode::ContinuousDelivery, default_strategies());
    let mut b: BTreeMap<String, BranchConfiguration> = BTreeMap::new();

    b.insert(
        "main".into(),
        BranchConfiguration {
            increment: Some(IncrementStrategy::Patch),
            label: Some(String::new()),
            source_branches: vec![],
            prevent_increment: Some(prevent(Some(true), None, None)),
            is_main_branch: Some(true),
            pre_release_weight: Some(55000),
            ..branch(MAIN_REGEX)
        },
    );
    b.insert(
        "release".into(),
        BranchConfiguration {
            increment: Some(IncrementStrategy::Patch),
            mode: Some(DeploymentMode::ManualDeployment),
            label: Some("beta".into()),
            source_branches: vec!["main".into()],
            prevent_increment: Some(prevent(Some(true), Some(false), Some(false))),
            track_merge_target: Some(false),
            track_merge_message: Some(true),
            is_release_branch: Some(true),
            pre_release_weight: Some(30000),
            ..branch(RELEASE_REGEX)
        },
    );
    b.insert(
        "feature".into(),
        BranchConfiguration {
            increment: Some(IncrementStrategy::Inherit),
            mode: Some(DeploymentMode::ManualDeployment),
            label: Some("{BranchName}".into()),
            source_branches: vec!["main".into(), "release".into()],
            prevent_increment: Some(prevent(None, None, Some(false))),
            pre_release_weight: Some(30000),
            ..branch(FEATURE_REGEX)
        },
    );
    b.insert(
        "pull-request".into(),
        BranchConfiguration {
            increment: Some(IncrementStrategy::Inherit),
            mode: Some(DeploymentMode::ContinuousDelivery),
            label: Some("PullRequest{Number}".into()),
            source_branches: vec!["main".into(), "release".into(), "feature".into()],
            prevent_increment: Some(prevent(Some(true), None, Some(false))),
            pre_release_weight: Some(30000),
            ..branch(PR_REGEX)
        },
    );
    b.insert(
        "unknown".into(),
        BranchConfiguration {
            increment: Some(IncrementStrategy::Inherit),
            mode: Some(DeploymentMode::ManualDeployment),
            label: Some("{BranchName}".into()),
            source_branches: vec![
                "main".into(),
                "release".into(),
                "feature".into(),
                "pull-request".into(),
            ],
            prevent_increment: Some(prevent(None, None, Some(false))),
            track_merge_message: Some(false),
            ..branch(UNKNOWN_REGEX)
        },
    );

    c.branches = b;
    c
}

/// TrunkBased (Mainline) workflow.
pub fn trunkbased() -> GitVersionConfiguration {
    let mut c = global_base(
        DeploymentMode::ContinuousDelivery,
        vec![
            VersionStrategy::ConfiguredNextVersion,
            VersionStrategy::Mainline,
        ],
    );
    let mut b: BTreeMap<String, BranchConfiguration> = BTreeMap::new();

    b.insert(
        "main".into(),
        BranchConfiguration {
            increment: Some(IncrementStrategy::Patch),
            mode: Some(DeploymentMode::ContinuousDeployment),
            label: Some(String::new()),
            source_branches: vec![],
            prevent_increment: Some(prevent(Some(true), None, None)),
            is_main_branch: Some(true),
            pre_release_weight: Some(55000),
            ..branch(MAIN_REGEX)
        },
    );
    b.insert(
        "feature".into(),
        BranchConfiguration {
            increment: Some(IncrementStrategy::Minor),
            mode: Some(DeploymentMode::ContinuousDelivery),
            label: Some("{BranchName}".into()),
            source_branches: vec!["main".into()],
            prevent_increment: Some(prevent(None, None, Some(false))),
            pre_release_weight: Some(30000),
            ..branch(FEATURE_REGEX)
        },
    );
    b.insert(
        "hotfix".into(),
        BranchConfiguration {
            increment: Some(IncrementStrategy::Patch),
            mode: Some(DeploymentMode::ContinuousDelivery),
            label: Some("{BranchName}".into()),
            source_branches: vec!["main".into()],
            prevent_increment: Some(prevent(None, None, Some(false))),
            is_release_branch: Some(true),
            pre_release_weight: Some(30000),
            ..branch(HOTFIX_REGEX)
        },
    );
    b.insert(
        "pull-request".into(),
        BranchConfiguration {
            increment: Some(IncrementStrategy::Inherit),
            mode: Some(DeploymentMode::ContinuousDelivery),
            label: Some("PullRequest{Number}".into()),
            source_branches: vec!["main".into(), "feature".into(), "hotfix".into()],
            prevent_increment: Some(prevent(Some(true), None, Some(false))),
            pre_release_weight: Some(30000),
            ..branch(PR_REGEX)
        },
    );
    b.insert(
        "unknown".into(),
        BranchConfiguration {
            increment: Some(IncrementStrategy::Patch),
            mode: Some(DeploymentMode::ContinuousDelivery),
            label: Some("{BranchName}".into()),
            source_branches: vec!["main".into()],
            prevent_increment: Some(prevent(None, None, Some(false))),
            pre_release_weight: Some(30000),
            ..branch(UNKNOWN_REGEX)
        },
    );

    c.branches = b;
    c
}

/// Select default configuration by workflow name. Falls back to GitFlow when None.
pub fn for_workflow(workflow: Option<&str>) -> GitVersionConfiguration {
    match workflow.map(|w| w.to_ascii_lowercase()) {
        Some(w) if w.starts_with("githubflow") => githubflow(),
        Some(w) if w.starts_with("trunkbased") || w.starts_with("mainline") => trunkbased(),
        _ => gitflow(),
    }
}
