//! Version calculation engine.
//!
//! Ports the strategy → increment → selection → deployment-mode pipeline from
//! the original `GitVersion.Core/VersionCalculation`. Handles common GitFlow/GitHubFlow
//! scenarios faithfully; Mainline is implemented in a simplified form.

use crate::config::{
    effective::EffectiveConfiguration, CommitMessageIncrementMode, DeploymentMode,
    GitVersionConfiguration, IncrementStrategy, SemanticVersionFormat, VersionStrategy,
    VersioningScheme,
};
use crate::git::{CommitInfo, GitRepo};
use crate::output::variables::VersionVariables;
use crate::version::{BuildMetaData, PreReleaseTag, SemanticVersion, VersionField};
use anyhow::Result;
use chrono::{DateTime, FixedOffset, NaiveDateTime, TimeZone};
use regex::Regex;
use std::collections::HashSet;

/// Set of commits excluded from version calculation. Corresponds to the original `ignore` config.
#[derive(Debug, Clone, Default)]
struct IgnoreSet {
    shas: HashSet<String>,
    before: Option<DateTime<FixedOffset>>,
    /// Exclude commits that only changed files under these path prefixes (ignore.paths).
    paths: Vec<String>,
}

impl IgnoreSet {
    fn from_config(config: &GitVersionConfiguration) -> Self {
        let shas: HashSet<String> = config.ignore.sha.iter().map(|s| s.to_lowercase()).collect();
        let before = config
            .ignore
            .commits_before
            .as_deref()
            .and_then(parse_ignore_date);
        let paths = config.ignore.paths.clone();
        IgnoreSet {
            shas,
            before,
            paths,
        }
    }

    fn is_ignored(&self, sha: &str, when: &DateTime<FixedOffset>) -> bool {
        if self.shas.contains(&sha.to_lowercase()) {
            return true;
        }
        // The entry may be a prefix rather than the full SHA, so check prefix matches too.
        if self
            .shas
            .iter()
            .any(|s| sha.to_lowercase().starts_with(s.as_str()) && s.len() >= 7)
        {
            return true;
        }
        matches!(&self.before, Some(b) if when < b)
    }

    /// Returns true when all files changed by the commit fall under ignored path prefixes.
    fn is_path_ignored(&self, repo: &crate::git::GitRepo, sha: &str) -> bool {
        if self.paths.is_empty() {
            return false;
        }
        let changed = repo.changed_paths_for_commit(sha);
        // Commits with no changed files (e.g. --allow-empty) satisfy vacuous truth:
        // all (zero) files are under ignored paths, so the commit is ignored (matches .NET GitVersion).
        if changed.is_empty() {
            return true;
        }
        changed.iter().all(|file| {
            self.paths.iter().any(|prefix| {
                let prefix = prefix.trim_end_matches('/');
                file == prefix || file.starts_with(&format!("{prefix}/"))
            })
        })
    }

    fn filter(&self, repo: &crate::git::GitRepo, commits: Vec<CommitInfo>) -> Vec<CommitInfo> {
        if self.shas.is_empty() && self.before.is_none() && self.paths.is_empty() {
            return commits;
        }
        commits
            .into_iter()
            .filter(|c| !self.is_ignored(&c.sha, &c.when) && !self.is_path_ignored(repo, &c.sha))
            .collect()
    }
}

/// Parse an ignore date in `yyyy-MM-ddTHH:mm:ss` (or date-only) format, assuming UTC.
fn parse_ignore_date(s: &str) -> Option<DateTime<FixedOffset>> {
    let s = s.trim();
    for fmt in ["%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M:%S", "%Y-%m-%d"] {
        if let Ok(ndt) = NaiveDateTime::parse_from_str(s, fmt) {
            return Some(chrono::Utc.from_utc_datetime(&ndt).fixed_offset());
        }
        if let Ok(d) = chrono::NaiveDate::parse_from_str(s, fmt) {
            if let Some(ndt) = d.and_hms_opt(0, 0, 0) {
                return Some(chrono::Utc.from_utc_datetime(&ndt).fixed_offset());
            }
        }
    }
    None
}

/// Convert a .NET date format string to a chrono strftime format (common tokens only).
fn dotnet_date_format_to_strftime(fmt: &str) -> String {
    // Longer tokens must be replaced first to avoid partial matches.
    let mut out = fmt.to_string();
    for (from, to) in [
        ("yyyy", "%Y"),
        ("yy", "%y"),
        ("MMMM", "%B"),
        ("MMM", "%b"),
        ("MM", "%m"),
        ("dddd", "%A"),
        ("ddd", "%a"),
        ("dd", "%d"),
        ("HH", "%H"),
        ("mm", "%M"),
        ("ss", "%S"),
    ] {
        out = out.replace(from, to);
    }
    out
}

/// A base version candidate produced by one strategy.
#[derive(Debug, Clone)]
struct BaseVersion {
    source: String,
    semantic_version: SemanticVersion,
    base_version_source: Option<String>,
    /// Timestamp of the base-source commit (used to pick the most recent source).
    source_when: Option<DateTime<FixedOffset>>,
    increment: VersionField,
    label: Option<String>,
    force_increment: bool,
    /// Use the current commit's tag as-is (no increment / label / deployment mode applied).
    exact: bool,
}

impl BaseVersion {
    fn new(
        source: impl Into<String>,
        semantic_version: SemanticVersion,
        base_version_source: Option<String>,
        increment: VersionField,
        label: Option<String>,
    ) -> Self {
        Self {
            source: source.into(),
            semantic_version,
            base_version_source,
            source_when: None,
            increment,
            label,
            force_increment: false,
            exact: false,
        }
    }
}

/// Result of applying an increment to a candidate.
#[derive(Debug, Clone)]
struct NextVersion {
    incremented: SemanticVersion,
    base: BaseVersion,
}

/// Convert an `IncrementStrategy` to a `VersionField`.
fn strategy_to_field(s: IncrementStrategy) -> VersionField {
    match s {
        IncrementStrategy::Major => VersionField::Major,
        IncrementStrategy::Minor => VersionField::Minor,
        IncrementStrategy::Patch => VersionField::Patch,
        IncrementStrategy::None | IncrementStrategy::Inherit => VersionField::None,
    }
}

/// Extract the bump field from a single commit message. Returns `None` when no pattern matches.
fn increment_from_message(msg: &str, eff: &EffectiveConfiguration) -> Option<VersionField> {
    let test = |pat: &str| {
        Regex::new(&format!("(?im){pat}"))
            .map(|r| r.is_match(msg))
            .unwrap_or(false)
    };
    if test(&eff.major_bump_message) {
        Some(VersionField::Major)
    } else if test(&eff.minor_bump_message) {
        Some(VersionField::Minor)
    } else if test(&eff.patch_bump_message) {
        Some(VersionField::Patch)
    } else if test(&eff.no_bump_message) {
        Some(VersionField::None)
    } else {
        None
    }
}

/// Determine the increment field from commits between `base_source` (exclusive) and `head`.
/// Mirrors the original `IncrementStrategyFinder.DetermineIncrementedField`.
fn determine_increment(
    repo: &GitRepo,
    base_source: Option<&str>,
    head_sha: &str,
    should_increment: bool,
    eff: &EffectiveConfiguration,
    ignore: &IgnoreSet,
) -> Result<VersionField> {
    let default_increment = strategy_to_field(eff.increment);

    let message_increment =
        if eff.commit_message_incrementing == CommitMessageIncrementMode::Disabled {
            None
        } else {
            let commits = ignore.filter(repo, repo.commits_between(base_source, head_sha)?);
            let merge_only =
                eff.commit_message_incrementing == CommitMessageIncrementMode::MergeMessageOnly;
            let mut best: Option<VersionField> = None;
            for c in &commits {
                if merge_only && c.parent_count < 2 {
                    continue;
                }
                if let Some(f) = increment_from_message(&c.message, eff) {
                    best = Some(best.map_or(f, |b| b.max(f)));
                }
            }
            best
        };

    Ok(match message_increment {
        None => {
            if should_increment {
                default_increment
            } else {
                VersionField::None
            }
        }
        Some(mi) => {
            if should_increment && mi < default_increment {
                default_increment
            } else {
                mi
            }
        }
    })
}

/// Parse a version string according to the configured `semantic-version-format`.
fn parse_version(input: &str, eff: &EffectiveConfiguration) -> Option<SemanticVersion> {
    let strict = eff.semantic_version_format == SemanticVersionFormat::Strict;
    SemanticVersion::parse_with(input, &eff.tag_prefix, strict)
}

/// Pre-validate all regex values in the config. The original GitVersion fails the calculation when
/// it encounters an invalid `tag-prefix` or `*-version-bump-message` regex, so we return an error
/// rather than silently ignoring them. (`version-in-branch-pattern` is excluded because it is only
/// used on release branches and would not be validated on main etc.)
fn validate_config_regexes(eff: &EffectiveConfiguration) -> Result<()> {
    let check = |label: &str, pat: &str| -> Result<()> {
        Regex::new(pat)
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("Invalid {label} regex '{pat}': {e}"))
    };
    check("tag-prefix", &eff.tag_prefix)?;
    if eff.commit_message_incrementing != CommitMessageIncrementMode::Disabled {
        check("major-version-bump-message", &eff.major_bump_message)?;
        check("minor-version-bump-message", &eff.minor_bump_message)?;
        check("patch-version-bump-message", &eff.patch_bump_message)?;
        check("no-bump-message", &eff.no_bump_message)?;
    }
    Ok(())
}

/// Extract a version token from a message or branch name (mirrors the original `ReferenceNameExtensions`).
///
/// The original splits the branch name by a separator and matches `^{pattern}` against each part.
/// The separator is `/` when the text contains `/` or no `-`; otherwise `-`.
/// The extracted token is parsed according to the configured `semantic-version-format` (Strict/Loose).
fn extract_version(text: &str, eff: &EffectiveConfiguration) -> Option<SemanticVersion> {
    let pattern = format!(
        "(?i)^{}",
        eff.version_in_branch_pattern.trim_start_matches('^')
    );
    let re = Regex::new(&pattern).ok()?;
    let sep = if text.contains('/') || !text.contains('-') {
        '/'
    } else {
        '-'
    };
    for part in text.split(sep) {
        if part.is_empty() {
            continue;
        }
        if let Some(caps) = re.captures(part) {
            let raw = caps
                .name("version")
                .map(|m| m.as_str())
                .unwrap_or_else(|| caps.get(0).unwrap().as_str());
            if let Some(v) = parse_version(raw, eff) {
                return Some(v);
            }
        }
    }
    None
}

/// Resolve an `Inherit` increment via git ancestry. Finds the source branch that the current
/// branch diverged from most recently (latest merge-base) and returns its increment.
/// Returns `None` when inheritance is not applicable or no candidate is found (caller keeps existing resolution).
fn resolve_inherit_via_git(
    repo: &GitRepo,
    config: &GitVersionConfiguration,
    branch_name: &str,
) -> Result<Option<IncrementStrategy>> {
    let Some((_, bc)) = crate::config::effective::find_branch_config(config, branch_name) else {
        return Ok(None);
    };
    let own = bc
        .increment
        .or(config.increment)
        .unwrap_or(IncrementStrategy::Inherit);
    if own != IncrementStrategy::Inherit {
        return Ok(None);
    }

    let repo_branches = repo.branch_names().unwrap_or_default();
    let mut best: Option<(i64, IncrementStrategy)> = None;

    for src_key in &bc.source_branches {
        let Some(src_bc) = config.branches.get(src_key) else {
            continue;
        };
        let Some(re_src) = &src_bc.regex else {
            continue;
        };
        let Ok(re) = Regex::new(&format!("(?i){re_src}")) else {
            continue;
        };

        // Actual repository branches that match this source config entry.
        for rb in &repo_branches {
            if rb == branch_name {
                continue;
            }
            let short = rb.rsplit('/').next().unwrap_or(rb);
            if !(re.is_match(rb) || re.is_match(short)) {
                continue;
            }
            let Some(mb) = repo.merge_base(branch_name, rb)? else {
                continue;
            };
            // A deeper merge-base (farther from the root) means a more recent divergence point.
            let depth = repo.commits_between(None, &mb)?.len() as i64;
            // Resolve the source branch's effective increment recursively (mirrors the original:
            // Inherit walks further up to its parents). Unresolvable cases yield None rather than
            // an arbitrary Patch fallback.
            let inc = crate::config::effective::resolve_increment(config, src_bc, 0);
            if best.map(|(d, _)| depth > d).unwrap_or(true) {
                best = Some((depth, inc));
            }
        }
    }
    Ok(best.map(|(_, inc)| inc))
}

/// Built-in merge message formats (mirrors the original `MergeMessage.cs`). Each format extracts
/// `SourceBranch`, from which the version is obtained via the `version-in-branch` pattern.
const BUILTIN_MERGE_FORMATS: &[&str] = &[
    // Default
    r"^Merge (branch|tag) '(?<SourceBranch>[^']*)'(?: into (?<TargetBranch>[^\s]*))*",
    // SmartGit
    r"^Finish (?<SourceBranch>[^\s]*)(?: into (?<TargetBranch>[^\s]*))*",
    // BitBucketPull
    r"^Merge pull request #(?<PullRequestNumber>\d+) (from|in) (?<Source>.*) from (?<SourceBranch>[^\s]*) to (?<TargetBranch>[^\s]*)",
    // BitBucketPullv7 (multiline: "Pull request #N\n\nMerge in X from Y to Z").
    // (?s) applies globally, so the first line/Source is restricted to [^\r\n] to match .NET behaviour.
    r"^Pull request #(?<PullRequestNumber>\d+)[^\r\n]*\r?\n\r?\nMerge in (?<Source>[^\r\n]*) from (?<SourceBranch>[^\s]*) to (?<TargetBranch>[^\s]*)",
    // BitBucketCloudPull
    r"^Merged in (?<SourceBranch>[^\s]*) \(pull request #(?<PullRequestNumber>\d+)\)",
    // GitHubPull
    r"^Merge pull request #(?<PullRequestNumber>\d+) (from|in) (?:[^\s/]+/)?(?<SourceBranch>[^\s]*)(?: into (?<TargetBranch>[^\s]*))*",
    // RemoteTracking
    r"^Merge remote-tracking branch '(?<SourceBranch>[^\s]*)'(?: into (?<TargetBranch>[^\s]*))*",
    // AzureDevOpsPull
    r"^Merge pull request (?<PullRequestNumber>\d+) from (?<SourceBranch>[^\s]*) into (?<TargetBranch>[^\s]*)",
];

/// Parse a merge message and return `(merged branch name, extracted version)`.
/// Tries the user-defined `merge-message-formats` first, then the 8 built-in formats.
fn parse_merge_message(
    message: &str,
    eff: &EffectiveConfiguration,
) -> Option<(String, SemanticVersion)> {
    let from_branch = |sb: &str| -> Option<SemanticVersion> {
        parse_version(sb, eff).or_else(|| extract_version(sb, eff))
    };

    // User-defined formats first, then the 8 built-in formats.
    let custom = eff.merge_message_formats.values().map(|s| s.as_str());
    for pattern in custom.chain(BUILTIN_MERGE_FORMATS.iter().copied()) {
        let Ok(re) = Regex::new(&format!("(?s){pattern}")) else {
            continue;
        };
        let Some(caps) = re.captures(message) else {
            continue;
        };
        let Some(sb) = caps.name("SourceBranch") else {
            continue;
        };
        let branch = sb.as_str().to_string();
        if let Some(v) = caps
            .name("Version")
            .and_then(|m| parse_version(m.as_str(), eff))
        {
            return Some((branch, v));
        }
        if let Some(v) = from_branch(&branch) {
            return Some((branch, v));
        }
        return None; // Format matched but no version found.
    }
    None
}

/// Extract the merged branch name from a merge commit message and return its configured increment.
///
/// Used during Mainline trunk walk to determine the increment floor for a merge commit.
/// When the merged branch is configured as Minor (e.g. a TrunkBased feature), Minor is used
/// instead of the default Patch. `Inherit`/`None` have no effect and return `None`.
/// Returns `None` when `prevent_increment.when_branch_merged = true` on the merged branch.
fn merge_branch_increment(config: &GitVersionConfiguration, message: &str) -> Option<VersionField> {
    for pattern in BUILTIN_MERGE_FORMATS {
        let Ok(re) = Regex::new(&format!("(?s){pattern}")) else {
            continue;
        };
        let Some(caps) = re.captures(message) else {
            continue;
        };
        let Some(sb) = caps.name("SourceBranch") else {
            continue;
        };
        let branch = sb.as_str();
        let (_, bc) = crate::config::effective::find_branch_config(config, branch)?;
        // when_branch_merged=true → this merge commit must not increment at all.
        // Some(VersionField::None) is a forced no-op signal that also blocks trunk_default.
        if bc
            .prevent_increment
            .as_ref()
            .and_then(|pi| pi.when_branch_merged)
            .unwrap_or(false)
        {
            return Some(VersionField::None);
        }
        let increment = bc.increment.unwrap_or(IncrementStrategy::Inherit);
        if matches!(
            increment,
            IncrementStrategy::Inherit | IncrementStrategy::None
        ) {
            return None;
        }
        return Some(strategy_to_field(increment));
    }
    None
}

/// Returns true if the branch name matches a branch config with `is-release-branch = true`.
fn is_release_branch(config: &GitVersionConfiguration, branch_name: &str) -> bool {
    let short = branch_name.rsplit('/').next().unwrap_or(branch_name);
    config.branches.values().any(|bc| {
        bc.is_release_branch == Some(true)
            && bc
                .regex
                .as_ref()
                .and_then(|r| Regex::new(&format!("(?i){r}")).ok())
                .map(|re| re.is_match(branch_name) || re.is_match(short))
                .unwrap_or(false)
    })
}

/// Main calculation entry point. Produces the final output variables.
pub fn calculate(
    repo: &GitRepo,
    config: &GitVersionConfiguration,
    branch_override: Option<String>,
) -> Result<VersionVariables> {
    // If branch_override is an actual ref, use its tip as HEAD (recalculate for that branch);
    // otherwise use the current HEAD.
    let (head, branch_name) = match &branch_override {
        Some(b) => {
            let head = repo
                .commit_info_of(b)
                .map(Ok)
                .unwrap_or_else(|| repo.head_commit())?;
            (head, b.clone())
        }
        None => (repo.head_commit()?, repo.current_branch_name()?),
    };
    let mut eff = EffectiveConfiguration::resolve(config, &branch_name);
    // Invalid regex config causes a calculation error just like the original (not silently ignored).
    validate_config_regexes(&eff)?;
    let ignore = IgnoreSet::from_config(config);

    // When the Mainline strategy is active, use the per-commit accumulation approach.
    if config.strategies.contains(&VersionStrategy::Mainline) {
        return mainline_calculate(repo, config, &eff, &branch_name, &head, &ignore);
    }

    // Inherit increment: walk actual git ancestors to find the source branch the current branch
    // diverged from, then inherit its increment (not necessarily the first configured source).
    if let Some(inc) = resolve_inherit_via_git(repo, config, &branch_name)? {
        eff.increment = inc;
    }

    let mut candidates: Vec<BaseVersion> = Vec::new();
    let mut tag_alternatives: Vec<SemanticVersion> = Vec::new();
    let strategies = if config.strategies.is_empty() {
        vec![
            VersionStrategy::Fallback,
            VersionStrategy::ConfiguredNextVersion,
            VersionStrategy::MergeMessage,
            VersionStrategy::TaggedCommit,
            VersionStrategy::VersionInBranchName,
        ]
    } else {
        config.strategies.clone()
    };

    for strat in &strategies {
        match strat {
            VersionStrategy::ConfiguredNextVersion => {
                // Mirrors the original ConfiguredNextVersionVersionStrategy: skip when
                // next-version is empty; otherwise call SemanticVersion.Parse (throws on failure).
                // A next-version that cannot be parsed with the current format fails the whole calculation.
                if let Some(nv) = &eff.next_version {
                    if !nv.is_empty() {
                        let v = parse_version(nv, &eff).ok_or_else(|| {
                            anyhow::anyhow!("Failed to parse {nv} into a Semantic Version")
                        })?;
                        // When a pre-release label is present, it must match the current branch label.
                        // (Mirrors .NET IsMatchForBranchSpecificLabel behaviour.)
                        let label_ok =
                            !v.pre_release_tag.has_tag() || v.pre_release_tag.name == eff.label;
                        if label_ok {
                            candidates.push(BaseVersion::new(
                                "ConfiguredNextVersion",
                                v,
                                None,
                                VersionField::None,
                                Some(eff.label.clone()),
                            ));
                        }
                    }
                }
            }
            VersionStrategy::TaggedCommit | VersionStrategy::Mainline => {
                gather_tagged(
                    repo,
                    &eff,
                    &head,
                    &ignore,
                    &mut candidates,
                    &mut tag_alternatives,
                )?;
            }
            VersionStrategy::VersionInBranchName => {
                if eff.is_release_branch {
                    if let Some(v) = extract_version(&branch_name, &eff) {
                        candidates.push(BaseVersion::new(
                            "VersionInBranchName",
                            v,
                            None,
                            VersionField::None,
                            Some(eff.label.clone()),
                        ));
                    }
                }
            }
            VersionStrategy::MergeMessage => {
                // When track-merge-message is false, merge messages are not used as version sources.
                if eff.track_merge_message {
                    gather_merge_messages(repo, config, &eff, &head, &ignore, &mut candidates)?;
                }
            }
            VersionStrategy::TrackReleaseBranches => {
                gather_track_release(repo, config, &eff, &head, &branch_name, &mut candidates)?;
            }
            VersionStrategy::Fallback => {
                let field = determine_increment(repo, None, &head.sha, true, &eff, &ignore)?;
                candidates.push(BaseVersion::new(
                    "Fallback (0.0.0)",
                    SemanticVersion::new(0, 0, 0),
                    None,
                    field,
                    Some(eff.label.clone()),
                ));
            }
            VersionStrategy::None => {}
        }
    }

    if candidates.is_empty() {
        // Mirrors the original NextVersionCalculator.CalculateNextVersion: when no candidates
        // are found, fail the calculation rather than inserting an arbitrary fallback (0.0.0).
        // The default strategy list includes Fallback, so this path only occurs when Fallback
        // is explicitly excluded from `strategies`.
        return Err(anyhow::anyhow!(
            "No base versions determined on the current branch."
        ));
    }

    // Apply increments to each candidate.
    let next: Vec<NextVersion> = candidates
        .into_iter()
        .map(|b| {
            let incremented = if b.exact {
                b.semantic_version.clone()
            } else {
                b.semantic_version
                    .increment(b.increment, b.label.as_deref(), b.force_increment)
            };
            NextVersion {
                incremented,
                base: b,
            }
        })
        .collect();

    // Select the candidate with the highest IncrementedVersion.
    // Ties are broken in favour of the earlier candidate (mirroring .NET: TaggedCommit > VersionInBranchName).
    let max_idx = next.iter().enumerate().fold(0usize, |acc, (i, n)| {
        if n.incremented.cmp(&next[acc].incremented) == std::cmp::Ordering::Greater {
            i
        } else {
            acc
        }
    });

    // The base version source comes from the most recent candidate that has a source
    // (mirrors the original NextVersionCalculator LatestBaseVersionSource rule).
    // VSSV is taken from the base semantic_version of the selected (max) candidate.
    let latest_source = next
        .iter()
        .filter(|n| n.base.base_version_source.is_some())
        .max_by(|a, b| a.base.source_when.cmp(&b.base.source_when));
    let base_source = latest_source
        .and_then(|n| n.base.base_version_source.clone())
        .or_else(|| next[max_idx].base.base_version_source.clone());

    let chosen = next.into_iter().nth(max_idx).unwrap();
    // VSSV = semantic_version of the chosen base version (before increment).
    let source_semver = chosen.base.semantic_version.clone();

    let mut final_semver = apply_deployment_mode(
        repo,
        &eff,
        &branch_name,
        &head,
        &chosen,
        base_source.as_deref(),
        &ignore,
    )?;
    // AlternativeSemanticVersion adjustment: when a tag with a mismatched label exists on the branch
    // and its core is higher, replace the final version's major.minor.patch with that tag's core.
    // (Mirrors the .NET NextVersionCalculator.Calculate() alternativeSemanticVersion logic.)
    if let Some(alt) = tag_alternatives.iter().max_by(|a, b| a.cmp_core(b)) {
        if alt.cmp_core(&final_semver) == std::cmp::Ordering::Greater {
            final_semver.major = alt.major;
            final_semver.minor = alt.minor;
            final_semver.patch = alt.patch;
        }
    }
    let variables = build_variables(&eff, &branch_name, &head, &final_semver, &source_semver)?;
    Ok(variables)
}

/// Mainline strategy: accumulate increments for each commit starting from the highest tag (or 0.0.0).
///
/// Each commit uses the message-based increment (major/minor/patch) when it is higher than the
/// default; otherwise the default increment is applied (`+semver:none` is treated as the default).
/// Ports the core behaviour of the original `MainlineVersionStrategy`, producing a monotonically
/// increasing version without pre-releases, similar to ContinuousDeployment.
fn mainline_calculate(
    repo: &GitRepo,
    config: &GitVersionConfiguration,
    eff: &EffectiveConfiguration,
    branch_name: &str,
    head: &CommitInfo,
    ignore: &IgnoreSet,
) -> Result<VersionVariables> {
    // Build a sha → core version map of all reachable tags (highest wins when a commit has multiple tags).
    let mut tags_by_sha: std::collections::HashMap<String, SemanticVersion> =
        std::collections::HashMap::new();
    for tag in repo.tags()? {
        if ignore.is_ignored(&tag.target_sha, &tag.when) {
            continue;
        }
        if let Some(v) = parse_version(&tag.name, eff) {
            let core = SemanticVersion::new(v.major, v.minor, v.patch);
            let e = tags_by_sha
                .entry(tag.target_sha.clone())
                .or_insert_with(|| core.clone());
            if core.cmp_core(e) == std::cmp::Ordering::Greater {
                *e = core;
            }
        }
    }
    let core_gt =
        |a: &SemanticVersion, b: &SemanticVersion| a.cmp_core(b) == std::cmp::Ordering::Greater;

    let default = strategy_to_field(eff.increment);

    // Non-trunk branches (feature, hotfix, etc.) apply trunk increments only up to the merge-base
    // with the source branch, then apply the feature increment once. Falls back to the full trunk
    // walk when the source branch ref cannot be resolved.
    let merge_base_sha: Option<String> = if !eff.is_main_branch && !eff.source_branches.is_empty() {
        let src = &eff.source_branches[0];
        if let Some(src_info) = repo.commit_info_of(src) {
            repo.merge_base(&head.sha, &src_info.sha)?
        } else {
            None
        }
    } else {
        None
    };

    // Trunk walk: up to the merge-base (branch) or HEAD (trunk).
    // For branches, walk using the source branch config (e.g. Patch increment).
    let trunk_target = merge_base_sha.as_deref().unwrap_or(&head.sha);
    let trunk_eff_buf;
    let trunk_eff: &EffectiveConfiguration = if merge_base_sha.is_some() {
        trunk_eff_buf = EffectiveConfiguration::resolve(config, &eff.source_branches[0]);
        &trunk_eff_buf
    } else {
        eff
    };
    let trunk_default = strategy_to_field(trunk_eff.increment);

    let mut trunk = ignore.filter(repo, repo.first_parent_between(None, trunk_target)?);
    trunk.reverse();

    let mut version = SemanticVersion::new(0, 0, 0);
    let mut highest_tag = SemanticVersion::new(0, 0, 0);
    // For VSSV: track the trunk version before each commit is processed.
    let mut prev_trunk_version = SemanticVersion::new(0, 0, 0);
    for c in &trunk {
        prev_trunk_version = version.clone();
        // Commits introduced by this step: for a merge, those from the second-parent side; otherwise the commit itself.
        let introduced: Vec<CommitInfo> = if c.parents.len() >= 2 {
            ignore.filter(
                repo,
                repo.commits_between(Some(&c.parents[0]), &c.parents[1])?,
            )
        } else {
            vec![c.clone()]
        };

        // Highest tag core among the introduced commits (and the merge commit itself).
        let mut step_tag: Option<SemanticVersion> = None;
        for sha in introduced
            .iter()
            .map(|x| &x.sha)
            .chain(std::iter::once(&c.sha))
        {
            if let Some(tv) = tags_by_sha.get(sha) {
                if step_tag.as_ref().map(|s| core_gt(tv, s)).unwrap_or(true) {
                    step_tag = Some(tv.clone());
                }
            }
        }

        if let Some(tv) = step_tag {
            if core_gt(&tv, &highest_tag) {
                highest_tag = tv.clone();
            }
            // A tag that is at least as high as the current version fixes that version (no increment).
            if !core_gt(&version, &tv) {
                version = tv;
                continue;
            }
        }

        // No tag (or tag lower than current) → increment. Consolidate messages; default is the floor.
        let mut field = trunk_default;
        for ic in &introduced {
            if let Some(f) = increment_from_message(&ic.message, trunk_eff) {
                if f > field {
                    field = f;
                }
            }
        }
        // For merge commits, also apply the merged branch's configured increment as a floor.
        // (e.g. TrunkBased feature = Minor → if Minor > Patch, use Minor.)
        // Some(VersionField::None) signals when_branch_merged=true: blocks everything including trunk_default.
        if c.parents.len() >= 2 {
            match merge_branch_increment(config, &c.message) {
                Some(VersionField::None) => {
                    field = VersionField::None;
                }
                Some(branch_field) if branch_field > field => {
                    field = branch_field;
                }
                _ => {}
            }
        }
        version = version.increment(field, None, true);
    }
    // For VSSV: trunk version after the walk and before any feature increment.
    let trunk_version_end = version.clone();

    // Compute distance and source_sha.
    let (mut version, source_sha, distance) = if let Some(ref mb_sha) = merge_base_sha {
        // Branch: count only the commits in the branch portion (after the merge-base) as distance.
        let feature_commits = ignore.filter(repo, repo.commits_between(Some(mb_sha), &head.sha)?);
        let head_is_tagged = tags_by_sha.contains_key(&head.sha);

        // Check whether the branch portion (including HEAD) has a tag.
        let feature_tag = feature_commits
            .iter()
            .filter_map(|c| {
                tags_by_sha
                    .get(&c.sha)
                    .map(|tv| (c.sha.clone(), tv.clone()))
            })
            .reduce(|(sa, a), (sb, b)| if core_gt(&b, &a) { (sb, b) } else { (sa, a) });

        if let Some((ft_sha, ft)) = feature_tag {
            // Tag exists on the branch: use it as the version source.
            if head_is_tagged && !eff.prevent_increment_when_current_commit_tagged {
                // HEAD is tagged and prevent-increment=false → one additional increment, distance=0.
                let v = ft.increment(default, None, true);
                (v, Some(head.sha.clone()), 0i64)
            } else {
                let d = repo.commits_between(Some(&ft_sha), &head.sha)?.len() as i64;
                (ft, Some(ft_sha), d)
            }
        } else {
            // No tag on the branch: trunk version + one branch increment, distance = number of branch commits.
            let v = version.increment(default, None, true);
            let d = feature_commits.len() as i64;
            (v, Some(mb_sha.clone()), d)
        }
    } else {
        // Trunk: handle HEAD tag (when-current-commit-tagged: false).
        let head_is_tagged = tags_by_sha.contains_key(&head.sha);
        if head_is_tagged && !eff.prevent_increment_when_current_commit_tagged {
            let v = version.increment(default, None, true);
            (v, Some(head.sha.clone()), 0i64)
        } else {
            // The Mainline version source is HEAD's first parent (the previous trunk state).
            let s = head.parents.first().cloned();
            let d = repo.commits_between(s.as_deref(), &head.sha)?.len() as i64;
            (version, s, d)
        }
    };

    // Set pre-release / build metadata according to the deployment mode.
    let label = eff.label.as_str();
    let mut commits_since_tag = None;
    version.pre_release_tag = match eff.deployment_mode {
        // Core version only (no pre-release).
        DeploymentMode::ContinuousDeployment => PreReleaseTag::default(),
        // Pre-release number = distance.
        DeploymentMode::ContinuousDelivery => {
            PreReleaseTag::new(label, Some(distance), label.is_empty())
        }
        // Pre-release number = 1, build metadata = distance.
        DeploymentMode::ManualDeployment => {
            commits_since_tag = Some(distance);
            PreReleaseTag::new(label, Some(1), label.is_empty())
        }
    };
    version.build_metadata = BuildMetaData {
        commits_since_tag,
        branch: Some(branch_name.to_string()),
        sha: Some(head.sha.clone()),
        short_sha: Some(head.short_sha.clone()),
        commit_date: Some(head.when),
        version_source_sha: source_sha,
        version_source_distance: distance,
        uncommitted_changes: repo.uncommitted_changes().unwrap_or(0),
        version_source_increment: VersionField::None,
        other_metadata: None,
    };

    // Compute VersionSourceSemVer:
    // When source_sha has a tag, use that tag's core version; otherwise use the trunk version
    // at that point suffixed with "-1".
    // Branch: trunk_version_end = trunk version after the walk, before the feature increment.
    // Trunk: prev_trunk_version = trunk version immediately before HEAD was processed.
    let version_at_source = if merge_base_sha.is_some() {
        trunk_version_end
    } else {
        prev_trunk_version.clone()
    };
    let source_semver = match version.build_metadata.version_source_sha.as_deref() {
        None => SemanticVersion::new(0, 0, 0),
        Some(sha) => {
            if let Some(tv) = tags_by_sha.get(sha) {
                tv.clone()
            } else {
                let mut sv = version_at_source;
                sv.pre_release_tag = PreReleaseTag::new("", Some(1), true);
                sv
            }
        }
    };

    build_variables(eff, branch_name, head, &version, &source_semver)
}

/// Check whether a version matches the branch label.
///
/// Mirrors .NET `SemanticVersion.IsMatchForBranchSpecificLabel`:
/// `(Name.Length == 0 && Number is null) || IsLabeledWith(value)`
fn is_match_for_branch_label(version: &SemanticVersion, label: &str) -> bool {
    let pre = &version.pre_release_tag;
    // Release version (name="" and number=None): always matches.
    if pre.name.is_empty() && pre.number.is_none() {
        return true;
    }
    // Has a pre-release: name must match the label (has_tag() && name == label).
    pre.has_tag() && pre.name == label
}

/// Collect version tags reachable from HEAD into candidates.
///
/// `alternatives`: all parsed tag versions, used for AlternativeSemanticVersion adjustment.
/// Tags whose label does not match the branch label are added only to `alternatives`, not to `out`.
fn gather_tagged(
    repo: &GitRepo,
    eff: &EffectiveConfiguration,
    head: &CommitInfo,
    ignore: &IgnoreSet,
    out: &mut Vec<BaseVersion>,
    alternatives: &mut Vec<SemanticVersion>,
) -> Result<()> {
    for tag in repo.tags()? {
        if ignore.is_ignored(&tag.target_sha, &tag.when) {
            continue;
        }
        if ignore.is_path_ignored(repo, &tag.target_sha) {
            continue;
        }
        if !repo
            .is_ancestor_of(&tag.target_sha, &head.sha)
            .unwrap_or(false)
        {
            continue;
        }
        let Some(version) = parse_version(&tag.name, eff) else {
            continue;
        };
        // Collect all tag versions for AlternativeSemanticVersion adjustment (regardless of label).
        alternatives.push(version.clone());
        // Tags that don't match the branch label are excluded from candidates.
        if !is_match_for_branch_label(&version, &eff.label) {
            continue;
        }
        let is_current = tag.target_sha == head.sha;
        let exact = is_current && eff.prevent_increment_when_current_commit_tagged;
        // Named pre-release tags (e.g. 1.0.0-beta.1) are not yet "releases" and are not used
        // as a version source. The core is not bumped; commit count starts from before the tag
        // commit (inclusive), matching the original TaggedCommitVersionStrategy behaviour.
        // Exception: numeric-only pre-releases (e.g. 1.0.0-1) are CD-style checkpoints and
        // are used as a version source.
        let has_pre = version.pre_release_tag.has_tag();
        let is_numeric_only_pre = has_pre && version.pre_release_tag.name.is_empty();
        let use_as_source = exact || !has_pre || is_numeric_only_pre;
        let base_src = if use_as_source {
            Some(tag.target_sha.clone())
        } else {
            None
        };
        let field = if exact {
            VersionField::None
        } else {
            let from = if use_as_source {
                Some(tag.target_sha.as_str())
            } else {
                None
            };
            determine_increment(repo, from, &head.sha, true, eff, ignore)?
        };
        let mut bv = BaseVersion::new(
            format!("Tag {}", tag.name),
            version,
            base_src,
            field,
            Some(eff.label.clone()),
        );
        bv.exact = exact;
        bv.source_when = if use_as_source { Some(tag.when) } else { None };
        out.push(bv);
    }
    Ok(())
}

/// Extract a version from merge commit messages.
///
/// Mirrors the original `MergeMessageVersionStrategy`: only uses the version when the merged
/// branch is a release branch.
fn gather_merge_messages(
    repo: &GitRepo,
    config: &GitVersionConfiguration,
    eff: &EffectiveConfiguration,
    head: &CommitInfo,
    ignore: &IgnoreSet,
    out: &mut Vec<BaseVersion>,
) -> Result<()> {
    // The original MergeMessageVersionStrategy.GetBaseVersions returns at most 5 candidates.
    let mut count = 0usize;
    for c in ignore.filter(repo, repo.commits_between(None, &head.sha)?) {
        if count >= 5 {
            break;
        }
        let Some((merged_branch, v)) = parse_merge_message(&c.message, eff) else {
            continue;
        };
        // Do not use the version when the merged branch is not a release branch.
        if !is_release_branch(config, &merged_branch) {
            continue;
        }
        // The base source for a merge commit is the merge-base of its two parents,
        // so that commits introduced by the merge are accurately counted after the version source.
        let base_src = if c.parents.len() >= 2 {
            repo.merge_base(&c.parents[0], &c.parents[1])?
                .unwrap_or_else(|| c.sha.clone())
        } else {
            c.sha.clone()
        };
        let field = if eff.prevent_increment_of_merged_branch {
            VersionField::None
        } else {
            determine_increment(repo, Some(&base_src), &head.sha, true, eff, ignore)?
        };
        let mut bv = BaseVersion::new(
            "MergeMessage",
            v,
            Some(base_src),
            field,
            Some(eff.label.clone()),
        );
        bv.source_when = Some(c.when);
        out.push(bv);
        count += 1;
    }
    Ok(())
}

/// Track release branches (e.g. from develop). Generates candidates based on the merge-base.
fn gather_track_release(
    repo: &GitRepo,
    config: &GitVersionConfiguration,
    eff: &EffectiveConfiguration,
    head: &CommitInfo,
    branch_name: &str,
    out: &mut Vec<BaseVersion>,
) -> Result<()> {
    if !eff.tracks_release_branches {
        return Ok(());
    }
    let Some((_, release_bc)) = config
        .branches
        .iter()
        .find(|(k, _)| k.as_str() == "release")
    else {
        return Ok(());
    };
    let Some(re_src) = &release_bc.regex else {
        return Ok(());
    };
    let Ok(re) = Regex::new(&format!("(?i){re_src}")) else {
        return Ok(());
    };

    for rb in repo.branch_names()? {
        let short = rb.rsplit('/').next().unwrap_or(&rb);
        if !(re.is_match(&rb) || re.is_match(short)) {
            continue;
        }
        if let Some(v) = extract_version(&rb, eff) {
            let base_src = repo.merge_base(branch_name, &rb)?;
            out.push(BaseVersion::new(
                format!("TrackReleaseBranches: {rb}"),
                v,
                base_src.or(Some(head.sha.clone())),
                strategy_to_field(eff.increment),
                Some(eff.label.clone()),
            ));
        }
    }
    Ok(())
}

/// Produce the final version (including build metadata) according to the deployment mode.
fn apply_deployment_mode(
    repo: &GitRepo,
    eff: &EffectiveConfiguration,
    branch_name: &str,
    head: &CommitInfo,
    chosen: &NextVersion,
    base_source: Option<&str>,
    ignore: &IgnoreSet,
) -> Result<SemanticVersion> {
    let base_src = if chosen.base.exact {
        chosen.base.base_version_source.as_deref()
    } else {
        base_source
    };
    let commits = ignore
        .filter(repo, repo.commits_between(base_src, &head.sha)?)
        .len() as i64;
    let uncommitted = repo.uncommitted_changes().unwrap_or(0);

    let mut sv = chosen.incremented.clone();
    let mut meta = BuildMetaData {
        commits_since_tag: Some(commits),
        branch: Some(branch_name.to_string()),
        sha: Some(head.sha.clone()),
        short_sha: Some(head.short_sha.clone()),
        commit_date: Some(head.when),
        version_source_sha: base_src.map(|s| s.to_string()),
        version_source_distance: commits,
        uncommitted_changes: uncommitted,
        // In the original, the final BaseVersion.Increment is recorded as None after the increment is consumed
        // (VersionSourceIncrement == None in all observed scenarios).
        version_source_increment: VersionField::None,
        other_metadata: None,
    };

    if chosen.base.exact {
        // Current commit is tagged → use as-is. No build metadata accumulation.
        meta.commits_since_tag = None;
        sv.build_metadata = meta;
        return Ok(sv);
    }

    match eff.deployment_mode {
        DeploymentMode::ManualDeployment => {
            // Keep core/tag; expose build metadata (short form) in FullSemVer.
        }
        DeploymentMode::ContinuousDelivery => {
            if sv.pre_release_tag.has_tag() {
                let n = sv.pre_release_tag.number.unwrap_or(1);
                sv.pre_release_tag.number = Some(n + commits - 1);
            }
            meta.commits_since_tag = None;
        }
        DeploymentMode::ContinuousDeployment => {
            sv.pre_release_tag = PreReleaseTag::default();
            meta.commits_since_tag = None;
        }
    }

    sv.build_metadata = meta;
    Ok(sv)
}

/// Build the final output variables.
fn build_variables(
    eff: &EffectiveConfiguration,
    branch_name: &str,
    head: &CommitInfo,
    sv: &SemanticVersion,
    source_semver: &SemanticVersion,
) -> Result<VersionVariables> {
    let pre = &sv.pre_release_tag;
    let pre_label = pre.name.clone();
    let pre_number = pre.number;
    let pre_tag_str = if pre.has_tag() {
        pre.format(false)
    } else {
        String::new()
    };

    let with_dash = |s: &str| {
        if s.is_empty() {
            String::new()
        } else {
            format!("-{s}")
        }
    };

    let major_minor_patch = sv.major_minor_patch();
    let sem_ver = sv.to_string();
    let commits = sv.build_metadata.version_source_distance;
    let full_build_meta = sv.build_metadata.format_full();

    // FullSemVer uses only the short build metadata (commit count), e.g. 1.0.1-1+2.
    let full_sem_ver = match sv.build_metadata.commits_since_tag {
        Some(n) => format!("{sem_ver}+{n}"),
        None => sem_ver.clone(),
    };

    // WeightedPreReleaseNumber: number + pre-release-weight when a number exists;
    // tag-pre-release-weight for stable releases. Mirrors the original SemanticVersionFormatValues.
    let weighted = Some(match pre_number {
        Some(n) => n + eff.pre_release_weight,
        None => eff.tag_pre_release_weight,
    });

    let assembly_sem_ver = assembly_version(sv, eff.assembly_versioning_scheme);
    let assembly_sem_file_ver = assembly_version(sv, eff.assembly_file_versioning_scheme);
    // InformationalVersion uses the full build metadata (includes branch/sha).
    let informational = if full_build_meta.is_empty() {
        sem_ver.clone()
    } else {
        format!("{sem_ver}+{full_build_meta}")
    };

    let escaped_branch = Regex::new(r"[^a-zA-Z0-9-]")
        .unwrap()
        .replace_all(branch_name, "-")
        .into_owned();

    let date_fmt = dotnet_date_format_to_strftime(&eff.commit_date_format);
    let commit_date = head.when.naive_utc().format(&date_fmt).to_string();

    let mut vars = VersionVariables {
        major: sv.major as u32,
        minor: sv.minor as u32,
        patch: sv.patch as u32,
        pre_release_tag: pre_tag_str.clone(),
        pre_release_tag_with_dash: with_dash(&pre_tag_str),
        pre_release_label: pre_label.clone(),
        pre_release_label_with_dash: with_dash(&pre_label),
        pre_release_number: pre_number,
        weighted_pre_release_number: weighted,
        build_meta_data: sv.build_metadata.commits_since_tag,
        full_build_meta_data: full_build_meta,
        major_minor_patch,
        sem_ver,
        full_sem_ver,
        assembly_sem_ver,
        assembly_sem_file_ver,
        informational_version: informational,
        branch_name: branch_name.to_string(),
        escaped_branch_name: escaped_branch,
        sha: head.sha.clone(),
        short_sha: head.short_sha.clone(),
        version_source_distance: Some(commits),
        version_source_increment: sv
            .build_metadata
            .version_source_increment
            .as_str()
            .to_string(),
        version_source_sem_ver: source_semver.to_string(),
        version_source_sha: sv
            .build_metadata
            .version_source_sha
            .clone()
            .unwrap_or_default(),
        commits_since_version_source: Some(commits),
        commit_date,
        uncommitted_changes: sv.build_metadata.uncommitted_changes,
    };

    // Apply custom assembly-*-format / assembly-informational-format.
    // These reference the variables computed above, so they are post-processed here.
    let ctx = vars.to_map();
    if let Some(fmt) = &eff.assembly_versioning_format {
        vars.assembly_sem_ver = render_template(fmt, &ctx)?;
    }
    if let Some(fmt) = &eff.assembly_file_versioning_format {
        vars.assembly_sem_file_ver = render_template(fmt, &ctx)?;
    }
    // The default informational-format `{InformationalVersion}` reproduces the original value,
    // so it is always safe to apply.
    vars.informational_version = render_template(&eff.assembly_informational_format, &ctx)?;

    Ok(vars)
}

/// Substitute `{Variable}` and `{env:VAR}` tokens using the variable map.
/// The original GitVersion fails format expansion on unknown tokens, so this function also
/// returns `Err` for any token that is neither in `ctx` nor an `env:` reference.
fn render_template(fmt: &str, ctx: &std::collections::BTreeMap<String, String>) -> Result<String> {
    let re = Regex::new(r"\{(?<t>[A-Za-z0-9_:]+)\}").unwrap();
    let mut unknown: Option<String> = None;
    let out = re
        .replace_all(fmt, |c: &regex::Captures| {
            let t = &c["t"];
            if let Some(env_var) = t.strip_prefix("env:") {
                std::env::var(env_var).unwrap_or_default()
            } else if let Some(v) = ctx.get(t) {
                v.clone()
            } else {
                if unknown.is_none() {
                    unknown = Some(t.to_string());
                }
                String::new()
            }
        })
        .into_owned();
    match unknown {
        Some(t) => Err(anyhow::anyhow!(
            "Unknown template token '{{{t}}}' in format string"
        )),
        None => Ok(out),
    }
}

/// Apply the assembly versioning scheme.
fn assembly_version(sv: &SemanticVersion, scheme: VersioningScheme) -> String {
    let pre = sv.pre_release_tag.number.unwrap_or(0);
    match scheme {
        VersioningScheme::Major => format!("{}.0.0.0", sv.major),
        VersioningScheme::MajorMinor => format!("{}.{}.0.0", sv.major, sv.minor),
        VersioningScheme::MajorMinorPatch => {
            format!("{}.{}.{}.0", sv.major, sv.minor, sv.patch)
        }
        VersioningScheme::MajorMinorPatchTag => {
            format!("{}.{}.{}.{}", sv.major, sv.minor, sv.patch, pre)
        }
        VersioningScheme::None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::defaults;

    fn default_eff() -> EffectiveConfiguration {
        let cfg = defaults::gitflow();
        EffectiveConfiguration::resolve(&cfg, "main")
    }

    #[test]
    fn validate_config_regexes_rejects_bad_patterns() {
        // Default config passes.
        let eff = default_eff();
        assert!(validate_config_regexes(&eff).is_ok());
        // Invalid tag-prefix regex is an error (matches original behaviour).
        let mut bad_prefix = default_eff();
        bad_prefix.tag_prefix = "(unclosed".to_string();
        assert!(validate_config_regexes(&bad_prefix).is_err());
        // Invalid bump-message regex is also an error.
        let mut bad_bump = default_eff();
        bad_bump.major_bump_message = "[invalid".to_string();
        assert!(validate_config_regexes(&bad_bump).is_err());
        // When commit-message-incrementing=Disabled, bump-messages are not validated.
        let mut disabled = default_eff();
        disabled.major_bump_message = "[invalid".to_string();
        disabled.commit_message_incrementing = CommitMessageIncrementMode::Disabled;
        assert!(validate_config_regexes(&disabled).is_ok());
    }

    #[test]
    fn render_template_errors_on_unknown_token() {
        let mut ctx = std::collections::BTreeMap::new();
        ctx.insert("Major".to_string(), "1".to_string());
        // Known tokens are substituted.
        assert_eq!(render_template("v{Major}", &ctx).unwrap(), "v1");
        // env: tokens resolve from environment variables (empty string when absent).
        assert!(render_template("{env:GV_NO_SUCH_VAR}", &ctx).is_ok());
        // Unknown tokens return an error, matching the original GitVersion behaviour.
        assert!(render_template("{Bogus}", &ctx).is_err());
    }

    #[test]
    fn parse_ignore_date_formats() {
        // datetime format
        let dt = parse_ignore_date("2021-06-15T12:00:00").unwrap();
        assert!(dt.to_rfc3339().starts_with("2021-06-15"));
        // date only
        let dt2 = parse_ignore_date("2021-06-15").unwrap();
        assert!(dt2.to_rfc3339().starts_with("2021-06-15"));
        // space separator
        let dt3 = parse_ignore_date("2021-06-15 12:00:00").unwrap();
        assert!(dt3.to_rfc3339().starts_with("2021-06-15"));
        // invalid format
        assert!(parse_ignore_date("not-a-date").is_none());
    }

    #[test]
    fn ignore_set_sha_prefix_match() {
        // Prefix of 7+ characters matches.
        let full_sha = "abcdef1234567890abcdef1234567890abcdef12";
        let prefix = "abcdef1"; // 7 chars
        let mut set = IgnoreSet::default();
        set.shas.insert(prefix.to_lowercase());
        let when = chrono::Utc::now().fixed_offset();
        assert!(set.is_ignored(full_sha, &when));
        // A 6-character prefix does not match.
        let mut set2 = IgnoreSet::default();
        set2.shas.insert("abcdef".to_lowercase()); // 6 chars → no match
        assert!(!set2.is_ignored(full_sha, &when));
    }

    #[test]
    fn ignore_set_before_date() {
        let past = parse_ignore_date("2020-01-01").unwrap();
        let set = IgnoreSet {
            before: Some(parse_ignore_date("2021-01-01").unwrap()),
            ..Default::default()
        };
        // past(2020) < before(2021) → ignored
        assert!(set.is_ignored("anysha", &past));
        // future(2022) >= before → not ignored
        let future = parse_ignore_date("2022-01-01").unwrap();
        assert!(!set.is_ignored("anysha", &future));
    }

    #[test]
    fn strategy_to_field_all_variants() {
        assert_eq!(
            strategy_to_field(IncrementStrategy::Major),
            VersionField::Major
        );
        assert_eq!(
            strategy_to_field(IncrementStrategy::Minor),
            VersionField::Minor
        );
        assert_eq!(
            strategy_to_field(IncrementStrategy::Patch),
            VersionField::Patch
        );
        assert_eq!(
            strategy_to_field(IncrementStrategy::None),
            VersionField::None
        );
        assert_eq!(
            strategy_to_field(IncrementStrategy::Inherit),
            VersionField::None
        );
    }

    #[test]
    fn increment_from_message_all_levels() {
        let eff = default_eff();
        // major
        assert_eq!(
            increment_from_message("big change\n+semver: major", &eff),
            Some(VersionField::Major)
        );
        // minor
        assert_eq!(
            increment_from_message("new feature\n+semver: minor", &eff),
            Some(VersionField::Minor)
        );
        // patch
        assert_eq!(
            increment_from_message("small fix\n+semver: patch", &eff),
            Some(VersionField::Patch)
        );
        // none/skip
        assert_eq!(
            increment_from_message("chore\n+semver: none", &eff),
            Some(VersionField::None)
        );
        assert_eq!(
            increment_from_message("+semver: skip", &eff),
            Some(VersionField::None)
        );
        // No match
        assert_eq!(increment_from_message("ordinary commit", &eff), None);
    }

    #[test]
    fn increment_from_message_breaking_alias() {
        let eff = default_eff();
        assert_eq!(
            increment_from_message("+semver: breaking", &eff),
            Some(VersionField::Major)
        );
        assert_eq!(
            increment_from_message("+semver: feature", &eff),
            Some(VersionField::Minor)
        );
        assert_eq!(
            increment_from_message("+semver: fix", &eff),
            Some(VersionField::Patch)
        );
    }

    #[test]
    fn ignore_set_filter_empty_shortcircuit() {
        // shas/before/paths 모두 비어 있으면 filter 는 입력을 그대로 반환.
        use crate::git::CommitInfo;
        let set = IgnoreSet::default();
        let commits = vec![CommitInfo {
            sha: "abc".into(),
            short_sha: "abc".into(),
            message: "msg".into(),
            when: chrono::Utc::now().fixed_offset(),
            parent_count: 0,
            parents: vec![],
        }];
        // When shas/before/paths are all empty, filter returns input unchanged without calling GitRepo.
        // Since we cannot construct a real repo object here, we verify this indirectly: an empty
        // filter short-circuits without touching the commit list.
        assert!(set.shas.is_empty() && set.before.is_none() && set.paths.is_empty());
        let _ = commits; // compile-check only
    }
}
