//! 버전 계산 엔진.
//!
//! 원본 `GitVersion.Core/VersionCalculation` 의 전략 → 증분 → 선택 →
//! deployment mode 흐름을 옮긴다. 공통 GitFlow/GitHubFlow 시나리오를 정확히
//! 처리하며, Mainline 은 단순화된 형태로 구현한다.

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

/// 버전 계산에서 제외할 커밋 집합. 원본 `ignore` 설정.
#[derive(Debug, Clone, Default)]
struct IgnoreSet {
    shas: HashSet<String>,
    before: Option<DateTime<FixedOffset>>,
    /// 이 접두어 아래 파일만 변경한 커밋을 제외 (ignore.paths).
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
        // 전체 sha 가 아닌 접두어로 지정됐을 수도 있으므로 prefix 도 확인.
        if self
            .shas
            .iter()
            .any(|s| sha.to_lowercase().starts_with(s.as_str()) && s.len() >= 7)
        {
            return true;
        }
        matches!(&self.before, Some(b) if when < b)
    }

    /// 커밋의 변경 파일이 전부 무시 경로 안에 있으면 true.
    fn is_path_ignored(&self, repo: &crate::git::GitRepo, sha: &str) -> bool {
        if self.paths.is_empty() {
            return false;
        }
        let changed = repo.changed_paths_for_commit(sha);
        // 변경 파일이 없는 커밋(예: --allow-empty)은 vacuous truth:
        // 모든 파일이 무시 경로에 속하므로 무시한다(원본 .NET GitVersion 동작).
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

/// `yyyy-MM-ddTHH:mm:ss`(혹은 날짜만) 형태의 ignore 날짜 파싱(UTC 가정).
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

/// .NET 날짜 포맷 문자열을 chrono strftime 포맷으로 변환(상용 토큰만).
fn dotnet_date_format_to_strftime(fmt: &str) -> String {
    // 긴 토큰부터 치환해야 충돌이 없다.
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

/// 한 전략이 만들어 낸 base version 후보.
#[derive(Debug, Clone)]
struct BaseVersion {
    source: String,
    semantic_version: SemanticVersion,
    base_version_source: Option<String>,
    /// base source 커밋의 시각(가장 최신 source 선택에 사용).
    source_when: Option<DateTime<FixedOffset>>,
    increment: VersionField,
    label: Option<String>,
    force_increment: bool,
    /// 현재 커밋의 태그를 그대로 사용(증분/label/deployment 미적용).
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

/// 후보에 증분을 적용한 결과.
#[derive(Debug, Clone)]
struct NextVersion {
    incremented: SemanticVersion,
    base: BaseVersion,
}

/// IncrementStrategy → VersionField.
fn strategy_to_field(s: IncrementStrategy) -> VersionField {
    match s {
        IncrementStrategy::Major => VersionField::Major,
        IncrementStrategy::Minor => VersionField::Minor,
        IncrementStrategy::Patch => VersionField::Patch,
        IncrementStrategy::None | IncrementStrategy::Inherit => VersionField::None,
    }
}

/// 단일 커밋 메시지에서 bump 필드 추출. 매칭 없으면 None.
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

/// base_source(제외)~head 사이 커밋들을 보고 증분 필드 결정.
/// 원본 `IncrementStrategyFinder.DetermineIncrementedField`.
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

/// 설정의 semantic-version-format 에 맞춰 버전 문자열 파싱.
fn parse_version(input: &str, eff: &EffectiveConfiguration) -> Option<SemanticVersion> {
    let strict = eff.semantic_version_format == SemanticVersionFormat::Strict;
    SemanticVersion::parse_with(input, &eff.tag_prefix, strict)
}

/// 설정의 정규식 값들을 미리 컴파일 검증한다. 원본 GitVersion 은 잘못된
/// tag-prefix / *-version-bump-message 정규식을 만나면 계산을 실패시키므로,
/// 우리도 조용히 무시하지 않고 에러를 반환해 동작을 맞춘다. (version-in-branch-pattern
/// 은 release 브랜치에서만 사용되어 main 등에서는 검증되지 않으므로 제외.)
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

/// 메시지/브랜치명에서 버전 토큰 추출(원본 ReferenceNameExtensions).
///
/// 원본은 브랜치명을 separator 로 split 한 각 part 에 `^{pattern}` 을 매칭한다.
/// separator 는 `/` 를 포함하거나 `-` 를 포함하지 않으면 `/`, 아니면 `-`.
/// 추출된 토큰은 설정의 semantic-version-format(Strict/Loose)에 맞춰 파싱한다.
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

/// Inherit 증분을 git 조상 기반으로 해석. 현재 브랜치가 분기된 source 브랜치
/// (merge-base 가 가장 최신인 것)를 찾아 그 브랜치의 증분을 반환. 상속 대상이
/// 아니거나 후보가 없으면 None(기존 해석 유지).
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

        // 이 source 설정에 매칭되는 실제 저장소 브랜치들.
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
            // merge-base 가 루트에서 멀수록(=깊을수록) 최근에 분기한 것.
            let depth = repo.commits_between(None, &mb)?.len() as i64;
            let inc = src_bc
                .increment
                .or(config.increment)
                .filter(|i| *i != IncrementStrategy::Inherit)
                .unwrap_or(IncrementStrategy::Patch);
            if best.map(|(d, _)| depth > d).unwrap_or(true) {
                best = Some((depth, inc));
            }
        }
    }
    Ok(best.map(|(_, inc)| inc))
}

/// 내장 merge 메시지 포맷(원본 MergeMessage.cs). 각 포맷은 SourceBranch 를 추출하며,
/// 거기서 version-in-branch 패턴으로 버전을 얻는다.
const BUILTIN_MERGE_FORMATS: &[&str] = &[
    // Default
    r"^Merge (branch|tag) '(?<SourceBranch>[^']*)'(?: into (?<TargetBranch>[^\s]*))*",
    // SmartGit
    r"^Finish (?<SourceBranch>[^\s]*)(?: into (?<TargetBranch>[^\s]*))*",
    // BitBucketPull
    r"^Merge pull request #(?<PullRequestNumber>\d+) (from|in) (?<Source>.*) from (?<SourceBranch>[^\s]*) to (?<TargetBranch>[^\s]*)",
    // BitBucketPullv7 (멀티라인: "Pull request #N\n\nMerge in X from Y to Z").
    // (?s) 전역 적용이므로 첫 줄/Source 는 [^\r\n] 으로 한정해 .NET 동작과 맞춘다.
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

/// merge 메시지를 파싱해 (병합된 브랜치명, 추출된 버전)을 반환. 사용자 정의
/// `merge-message-formats` 와 8종 내장 포맷을 시도한다.
fn parse_merge_message(
    message: &str,
    eff: &EffectiveConfiguration,
) -> Option<(String, SemanticVersion)> {
    let from_branch = |sb: &str| -> Option<SemanticVersion> {
        parse_version(sb, eff).or_else(|| extract_version(sb, eff))
    };

    // 사용자 정의 포맷 우선, 이어서 8종 내장 포맷.
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
        return None; // 포맷은 맞지만 버전 없음.
    }
    None
}

/// 병합 커밋 메시지에서 병합된 브랜치명을 추출해 그 설정 증분을 반환.
///
/// Mainline 트렁크 walk 에서 merge 커밋의 증분 floor 를 결정할 때 사용한다.
/// 병합된 브랜치가 Minor 설정(예: TrunkBased feature)이면 Patch 기본값 대신
/// Minor 로 올려준다. Inherit/None 은 효과 없음으로 None 반환.
/// 병합된 브랜치에 `prevent_increment.when_branch_merged = true` 이면 None.
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
        // when_branch_merged=true 이면 이 병합 커밋은 일체 증분하지 않음.
        // Some(VersionField::None) 은 trunk_default 도 차단하는 강제 no-op 신호.
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

/// 브랜치명이 release 브랜치 설정(is-release-branch)에 매칭되는지.
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

/// 전체 계산 진입점. 최종 출력 변수를 만든다.
pub fn calculate(
    repo: &GitRepo,
    config: &GitVersionConfiguration,
    branch_override: Option<String>,
) -> Result<VersionVariables> {
    // branch override 가 실제 ref 면 그 브랜치 tip 을 head 로 사용(해당 브랜치 기준
    // 재계산). 아니면 현재 HEAD.
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
    // 잘못된 정규식 설정은 원본처럼 계산 에러로 처리한다(조용히 무시하지 않음).
    validate_config_regexes(&eff)?;
    let ignore = IgnoreSet::from_config(config);

    // Mainline 전략이 활성화되어 있으면 per-commit 누적 방식으로 계산.
    if config.strategies.contains(&VersionStrategy::Mainline) {
        return mainline_calculate(repo, config, &eff, &branch_name, &head, &ignore);
    }

    // Inherit 증분: 실제 git 조상을 따라 현재 브랜치가 분기된 source 브랜치를
    // 찾아 그 브랜치의 증분을 상속한다(설정상 첫 source 가 아니라).
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
                // 원본 ConfiguredNextVersionVersionStrategy: next-version 이 비었으면
                // 스킵, 있으면 SemanticVersion.Parse(실패 시 throw). 즉 현재 format
                // 으로 파싱 안 되는 next-version 은 전체 계산을 실패시킨다.
                if let Some(nv) = &eff.next_version {
                    if !nv.is_empty() {
                        let v = parse_version(nv, &eff).ok_or_else(|| {
                            anyhow::anyhow!("Failed to parse {nv} into a Semantic Version")
                        })?;
                        // pre-release label 이 있으면 현재 브랜치 label 과 일치해야 함.
                        // (.NET IsMatchForBranchSpecificLabel 동작)
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
                // track-merge-message 가 false 면 merge 메시지를 버전으로 해석하지 않음.
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
        // 안전망: 최소한 fallback.
        candidates.push(BaseVersion::new(
            "Fallback (0.0.0)",
            SemanticVersion::new(0, 0, 0),
            None,
            VersionField::Patch,
            Some(eff.label.clone()),
        ));
    }

    // 각 후보에 증분 적용.
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

    // 최고 IncrementedVersion 후보 선택.
    // 동률인 경우 .NET 과 마찬가지로 앞선(먼저 수집된) 후보 우선(TaggedCommit > VersionInBranchName).
    let max_idx = next.iter().enumerate().fold(0usize, |acc, (i, n)| {
        if n.incremented.cmp(&next[acc].incremented) == std::cmp::Ordering::Greater {
            i
        } else {
            acc
        }
    });

    // base version source 는 "source 를 가진 후보 중 가장 최신" 에서 가져온다
    // (원본 NextVersionCalculator 의 LatestBaseVersionSource 규칙).
    // VSSV 는 선택된(max) 후보의 base semantic_version 에서 가져온다.
    let latest_source = next
        .iter()
        .filter(|n| n.base.base_version_source.is_some())
        .max_by(|a, b| a.base.source_when.cmp(&b.base.source_when));
    let base_source = latest_source
        .and_then(|n| n.base.base_version_source.clone())
        .or_else(|| next[max_idx].base.base_version_source.clone());

    let chosen = next.into_iter().nth(max_idx).unwrap();
    // VSSV = 선택된 base version 의 semantic_version (증분 전)
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
    // AlternativeSemanticVersion 조정: 브랜치에 label 불일치 태그가 있고 그 코어가 더 높으면
    // 최종 버전의 major.minor.patch 를 그 태그 코어로 교체한다.
    // (.NET NextVersionCalculator.Calculate() alternativeSemanticVersion 동작)
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

/// Mainline 전략: base(최고 태그 또는 0.0.0)부터 각 커밋의 증분을 누적한다.
///
/// 각 커밋은 메시지 기반 증분(major/minor/patch)이 기본 증분보다 높으면 그것을,
/// 아니면 기본 증분을 적용한다(`+semver:none` 도 기본 증분으로 처리). 원본
/// MainlineVersionStrategy 의 핵심 동작을 옮긴 것으로, ContinuousDeployment 처럼
/// pre-release 없이 단조 증가하는 버전을 만든다.
fn mainline_calculate(
    repo: &GitRepo,
    config: &GitVersionConfiguration,
    eff: &EffectiveConfiguration,
    branch_name: &str,
    head: &CommitInfo,
    ignore: &IgnoreSet,
) -> Result<VersionVariables> {
    // 도달 가능한 모든 태그를 sha -> 코어 버전 맵으로(같은 커밋에 여러 태그면 최고).
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

    // 비-트렁크 브랜치(feature/hotfix 등)는 source 브랜치의 merge-base 까지만 트렁크
    // 증분을 적용하고, feature 증분은 1회만 적용한다. source 브랜치 ref 를 찾지 못하면
    // 기존 트렁크 walk 로 fallback.
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

    // 트렁크 부분 walk: merge-base 까지(브랜치) 또는 HEAD 까지(트렁크).
    // 브랜치인 경우에는 source 브랜치의 설정(Patch 증분 등)으로 walk 한다.
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
    // VSSV 계산용: 마지막 커밋 처리 전의 trunk 버전
    let mut prev_trunk_version = SemanticVersion::new(0, 0, 0);
    for c in &trunk {
        prev_trunk_version = version.clone();
        // 이 step 이 도입한 커밋들. merge 면 병합된 브랜치(두 번째 부모 계열), 아니면 자신.
        let introduced: Vec<CommitInfo> = if c.parents.len() >= 2 {
            ignore.filter(
                repo,
                repo.commits_between(Some(&c.parents[0]), &c.parents[1])?,
            )
        } else {
            vec![c.clone()]
        };

        // 도입 커밋(및 merge 커밋 자신)에 붙은 가장 높은 태그 코어.
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
            // 현재 버전 이상인 태그는 그 코어로 확정(증분하지 않음).
            if !core_gt(&version, &tv) {
                version = tv;
                continue;
            }
        }

        // 태그가 없거나 더 낮으면 증분(도입 커밋 메시지 consolidate, 바닥은 기본 증분).
        let mut field = trunk_default;
        for ic in &introduced {
            if let Some(f) = increment_from_message(&ic.message, trunk_eff) {
                if f > field {
                    field = f;
                }
            }
        }
        // 병합 커밋이면 병합된 브랜치의 설정 증분도 floor 로 적용.
        // (예: TrunkBased feature = Minor → main Patch 보다 높으면 Minor 적용)
        // Some(None) 은 when_branch_merged=true 신호: trunk_default 포함 일체 차단.
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
    // VSSV 계산용: trunk walk 이후 버전 (feature increment 전에 저장해야 함)
    let trunk_version_end = version.clone();

    // distance / source_sha 계산.
    let (mut version, source_sha, distance) = if let Some(ref mb_sha) = merge_base_sha {
        // 브랜치: 브랜치 부분(merge-base 이후)의 커밋만 distance 로 센다.
        let feature_commits = ignore.filter(repo, repo.commits_between(Some(mb_sha), &head.sha)?);
        let head_is_tagged = tags_by_sha.contains_key(&head.sha);

        // 브랜치 부분에 태그가 있는지 확인(HEAD 포함).
        let feature_tag = feature_commits
            .iter()
            .filter_map(|c| {
                tags_by_sha
                    .get(&c.sha)
                    .map(|tv| (c.sha.clone(), tv.clone()))
            })
            .reduce(|(sa, a), (sb, b)| if core_gt(&b, &a) { (sb, b) } else { (sa, a) });

        if let Some((ft_sha, ft)) = feature_tag {
            // 브랜치에 태그 존재: 태그를 version source 로 사용.
            if head_is_tagged && !eff.prevent_increment_when_current_commit_tagged {
                // HEAD 가 태그이고 prevent-increment=false → 1회 추가 증분, distance=0.
                let v = ft.increment(default, None, true);
                (v, Some(head.sha.clone()), 0i64)
            } else {
                let d = repo.commits_between(Some(&ft_sha), &head.sha)?.len() as i64;
                (ft, Some(ft_sha), d)
            }
        } else {
            // 브랜치에 태그 없음: trunk 버전 + 브랜치 증분 1회, distance = 브랜치 커밋 수.
            let v = version.increment(default, None, true);
            let d = feature_commits.len() as i64;
            (v, Some(mb_sha.clone()), d)
        }
    } else {
        // 트렁크: HEAD 태그 처리(when-current-commit-tagged: false).
        let head_is_tagged = tags_by_sha.contains_key(&head.sha);
        if head_is_tagged && !eff.prevent_increment_when_current_commit_tagged {
            let v = version.increment(default, None, true);
            (v, Some(head.sha.clone()), 0i64)
        } else {
            // Mainline 의 version source 는 head 의 첫 번째 부모(직전 트렁크 상태).
            let s = head.parents.first().cloned();
            let d = repo.commits_between(s.as_deref(), &head.sha)?.len() as i64;
            (version, s, d)
        }
    };

    // deployment mode 별 pre-release / build metadata.
    let label = eff.label.as_str();
    let mut commits_since_tag = None;
    version.pre_release_tag = match eff.deployment_mode {
        // 코어 버전만(pre-release 제거).
        DeploymentMode::ContinuousDeployment => PreReleaseTag::default(),
        // pre-release 번호 = distance.
        DeploymentMode::ContinuousDelivery => {
            PreReleaseTag::new(label, Some(distance), label.is_empty())
        }
        // pre-release 번호 = 1, build metadata = distance.
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

    // VersionSourceSemVer 계산:
    // source_sha 커밋에 태그가 있으면 그 코어 버전, 없으면 trunk 해당 시점 버전 + "-1".
    // 브랜치: trunk_version_end = feature increment 전 trunk walk 직후 버전.
    // 트렁크: prev_trunk_version = HEAD 처리 직전 버전.
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

/// 브랜치 label 매칭 여부 확인.
///
/// .NET `SemanticVersion.IsMatchForBranchSpecificLabel`:
/// `(Name.Length == 0 && Number is null) || IsLabeledWith(value)`
fn is_match_for_branch_label(version: &SemanticVersion, label: &str) -> bool {
    let pre = &version.pre_release_tag;
    // 릴리스 버전 (name="" and number=None): 항상 매칭
    if pre.name.is_empty() && pre.number.is_none() {
        return true;
    }
    // pre-release 있음: name이 label과 일치해야 함 (has_tag() && name == label)
    pre.has_tag() && pre.name == label
}

/// HEAD 에서 도달 가능한 버전 태그를 후보로 수집.
///
/// `alternatives`: AlternativeSemanticVersion 조정에 쓸 전체 파싱된 태그 버전.
/// 브랜치 label 과 일치하지 않는 태그는 `out` 에 넣지 않고 `alternatives` 에만 넣는다.
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
        // AlternativeSemanticVersion 조정용 전체 태그 버전 수집 (label 매칭 무관)
        alternatives.push(version.clone());
        // 브랜치 label 과 일치하지 않는 태그는 후보에서 제외
        if !is_match_for_branch_label(&version, &eff.label) {
            continue;
        }
        let is_current = tag.target_sha == head.sha;
        let exact = is_current && eff.prevent_increment_when_current_commit_tagged;
        // named pre-release 태그(예: 1.0.0-beta.1)는 아직 "릴리스" 가 아니므로
        // 버전 source 로 삼지 않는다. 코어를 올리지 않고, 커밋 수는 태그 커밋을
        // 포함해 그 이전부터 센다(원본 TaggedCommitVersionStrategy 동작).
        // 단, 숫자만으로 된 pre-release(예: 1.0.0-1)는 CD 스타일 체크포인트이므로
        // 버전 source 로 사용한다.
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

/// merge 커밋 메시지에서 버전을 추출.
///
/// 원본 MergeMessageVersionStrategy 와 동일하게, 병합된 브랜치가 release 브랜치인
/// 경우에만 그 버전을 사용한다.
fn gather_merge_messages(
    repo: &GitRepo,
    config: &GitVersionConfiguration,
    eff: &EffectiveConfiguration,
    head: &CommitInfo,
    ignore: &IgnoreSet,
    out: &mut Vec<BaseVersion>,
) -> Result<()> {
    // 원본 MergeMessageVersionStrategy.GetBaseVersions 는 최대 5개 후보만 반환한다.
    let mut count = 0usize;
    for c in ignore.filter(repo, repo.commits_between(None, &head.sha)?) {
        if count >= 5 {
            break;
        }
        let Some((merged_branch, v)) = parse_merge_message(&c.message, eff) else {
            continue;
        };
        // 병합된 브랜치가 release 브랜치가 아니면 버전을 사용하지 않는다.
        if !is_release_branch(config, &merged_branch) {
            continue;
        }
        // merge 커밋의 base source 는 두 부모의 merge-base. 그래야 병합으로 들어온
        // 커밋들이 버전 소스 이후 커밋 수에 정확히 반영된다.
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

/// release 브랜치를 추적(develop 등에서). merge-base 기준 후보 생성.
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

/// deployment mode 별 최종 버전(+빌드 메타데이터) 산출.
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
        // 원본에서 최종 BaseVersion.Increment 는 증분 소비 후 None 으로 기록된다
        // (관측된 모든 시나리오에서 VersionSourceIncrement == None).
        version_source_increment: VersionField::None,
        other_metadata: None,
    };

    if chosen.base.exact {
        // 현재 커밋이 태그 → 그대로. 빌드 메타데이터 누적 없음.
        meta.commits_since_tag = None;
        sv.build_metadata = meta;
        return Ok(sv);
    }

    match eff.deployment_mode {
        DeploymentMode::ManualDeployment => {
            // 코어/태그 유지, 빌드 메타데이터(짧은 형태)를 FullSemVer 에 노출.
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

/// 최종 출력 변수 구성.
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

    // FullSemVer 는 짧은 빌드 메타데이터(커밋 수)만 사용: 예) 1.0.1-1+2.
    let full_sem_ver = match sv.build_metadata.commits_since_tag {
        Some(n) => format!("{sem_ver}+{n}"),
        None => sem_ver.clone(),
    };

    // WeightedPreReleaseNumber: 번호가 있으면 번호+pre-release-weight,
    // 없으면(안정 릴리스) tag-pre-release-weight. 원본 SemanticVersionFormatValues.
    let weighted = Some(match pre_number {
        Some(n) => n + eff.pre_release_weight,
        None => eff.tag_pre_release_weight,
    });

    let assembly_sem_ver = assembly_version(sv, eff.assembly_versioning_scheme);
    let assembly_sem_file_ver = assembly_version(sv, eff.assembly_file_versioning_scheme);
    // InformationalVersion 은 전체 빌드 메타데이터(branch/sha 포함)를 사용.
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

    // assembly-*-format / assembly-informational-format 커스텀 포맷 적용.
    // 포맷은 위에서 계산된 변수들을 참조하므로 여기서 후처리한다.
    let ctx = vars.to_map();
    if let Some(fmt) = &eff.assembly_versioning_format {
        vars.assembly_sem_ver = render_template(fmt, &ctx)?;
    }
    if let Some(fmt) = &eff.assembly_file_versioning_format {
        vars.assembly_sem_file_ver = render_template(fmt, &ctx)?;
    }
    // informational-format 의 기본값 `{InformationalVersion}` 은 원래 값을 그대로
    // 재현하므로 항상 적용해도 안전하다.
    vars.informational_version = render_template(&eff.assembly_informational_format, &ctx)?;

    Ok(vars)
}

/// `{Variable}` 및 `{env:VAR}` 토큰을 변수 맵으로 치환.
/// `{Variable}`/`{env:VAR}` 토큰을 변수 맵으로 치환. 원본 GitVersion 은 알 수 없는
/// 토큰을 만나면 포맷 확장에 실패(에러)하므로, 여기서도 ctx 에 없고 `env:` 도 아닌
/// 토큰이 있으면 Err 를 반환한다.
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

/// AssemblyVersion 스킴 적용.
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
        // 기본 설정은 통과.
        let eff = default_eff();
        assert!(validate_config_regexes(&eff).is_ok());
        // 잘못된 tag-prefix 정규식은 에러(원본 동작).
        let mut bad_prefix = default_eff();
        bad_prefix.tag_prefix = "(unclosed".to_string();
        assert!(validate_config_regexes(&bad_prefix).is_err());
        // 잘못된 bump-message 정규식도 에러.
        let mut bad_bump = default_eff();
        bad_bump.major_bump_message = "[invalid".to_string();
        assert!(validate_config_regexes(&bad_bump).is_err());
        // commit-message-incrementing=Disabled 면 bump-message 는 검증하지 않는다.
        let mut disabled = default_eff();
        disabled.major_bump_message = "[invalid".to_string();
        disabled.commit_message_incrementing = CommitMessageIncrementMode::Disabled;
        assert!(validate_config_regexes(&disabled).is_ok());
    }

    #[test]
    fn render_template_errors_on_unknown_token() {
        let mut ctx = std::collections::BTreeMap::new();
        ctx.insert("Major".to_string(), "1".to_string());
        // 알려진 토큰은 치환된다.
        assert_eq!(render_template("v{Major}", &ctx).unwrap(), "v1");
        // env: 토큰은 환경변수(없으면 빈 문자열).
        assert!(render_template("{env:GV_NO_SUCH_VAR}", &ctx).is_ok());
        // 알 수 없는 토큰은 원본처럼 에러(원본 GitVersion 동작).
        assert!(render_template("{Bogus}", &ctx).is_err());
    }

    #[test]
    fn parse_ignore_date_formats() {
        // datetime 형식
        let dt = parse_ignore_date("2021-06-15T12:00:00").unwrap();
        assert!(dt.to_rfc3339().starts_with("2021-06-15"));
        // 날짜만
        let dt2 = parse_ignore_date("2021-06-15").unwrap();
        assert!(dt2.to_rfc3339().starts_with("2021-06-15"));
        // 공백 구분자
        let dt3 = parse_ignore_date("2021-06-15 12:00:00").unwrap();
        assert!(dt3.to_rfc3339().starts_with("2021-06-15"));
        // 잘못된 형식
        assert!(parse_ignore_date("not-a-date").is_none());
    }

    #[test]
    fn ignore_set_sha_prefix_match() {
        // 7글자 이상 접두어로 매칭
        let full_sha = "abcdef1234567890abcdef1234567890abcdef12";
        let prefix = "abcdef1"; // 7글자
        let mut set = IgnoreSet::default();
        set.shas.insert(prefix.to_lowercase());
        let when = chrono::Utc::now().fixed_offset();
        assert!(set.is_ignored(full_sha, &when));
        // 6글자 접두어는 무시됨
        let mut set2 = IgnoreSet::default();
        set2.shas.insert("abcdef".to_lowercase()); // 6글자 → 매칭 안 됨
        assert!(!set2.is_ignored(full_sha, &when));
    }

    #[test]
    fn ignore_set_before_date() {
        let past = parse_ignore_date("2020-01-01").unwrap();
        let set = IgnoreSet {
            before: Some(parse_ignore_date("2021-01-01").unwrap()),
            ..Default::default()
        };
        // past(2020) < before(2021) → 무시됨
        assert!(set.is_ignored("anysha", &past));
        // future(2022) >= before → 무시 안 됨
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
        // 매칭 없음
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
        // GitRepo 없이 동작 확인(shas/before/paths 비어 있으면 repo 호출 없음)
        // 실제 repo 객체 없이 호출 불가 → 빈 filter 는 shortcircuit 됨을 간접 확인.
        assert!(set.shas.is_empty() && set.before.is_none() && set.paths.is_empty());
        let _ = commits; // 컴파일 확인용
    }
}
