//! 버전 계산 엔진.
//!
//! 원본 `GitVersion.Core/VersionCalculation` 의 전략 → 증분 → 선택 →
//! deployment mode 흐름을 옮긴다. 공통 GitFlow/GitHubFlow 시나리오를 정확히
//! 처리하며, Mainline 은 단순화된 형태로 구현한다.

use crate::config::{
    effective::EffectiveConfiguration, CommitMessageConvention, CommitMessageIncrementMode,
    DeploymentMode, GitVersionConfiguration, IncrementStrategy, SemanticVersionFormat,
    VersionStrategy, VersioningScheme,
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
}

impl IgnoreSet {
    fn from_config(config: &GitVersionConfiguration) -> Self {
        let shas: HashSet<String> = config.ignore.sha.iter().map(|s| s.to_lowercase()).collect();
        let before = config.ignore.commits_before.as_deref().and_then(parse_ignore_date);
        IgnoreSet { shas, before }
    }

    fn is_ignored(&self, sha: &str, when: &DateTime<FixedOffset>) -> bool {
        if self.shas.contains(&sha.to_lowercase()) {
            return true;
        }
        // 전체 sha 가 아닌 접두어로 지정됐을 수도 있으므로 prefix 도 확인.
        if self.shas.iter().any(|s| sha.to_lowercase().starts_with(s.as_str()) && s.len() >= 7) {
            return true;
        }
        matches!(&self.before, Some(b) if when < b)
    }

    fn filter(&self, commits: Vec<CommitInfo>) -> Vec<CommitInfo> {
        if self.shas.is_empty() && self.before.is_none() {
            return commits;
        }
        commits.into_iter().filter(|c| !self.is_ignored(&c.sha, &c.when)).collect()
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

/// Conventional Commits 규약에서 증분 필드 추출.
///
/// - `feat!:` / `fix(scope)!:` 또는 `BREAKING CHANGE:` 푸터 → Major
/// - `feat:` → Minor
/// - `fix:` / `perf:` / `revert:` → Patch
/// - 그 외 type(docs/chore/...) 또는 비규약 메시지 → None(폴백)
fn conventional_increment(msg: &str) -> Option<VersionField> {
    let first = msg.lines().next().unwrap_or("");
    let header = Regex::new(r"^\s*(?P<type>[a-zA-Z]+)(\([^)]*\))?(?P<bang>!)?:").ok()?;
    let caps = header.captures(first)?;
    let breaking = caps.name("bang").is_some()
        || Regex::new(r"(?m)^BREAKING[ -]CHANGE:").map(|r| r.is_match(msg)).unwrap_or(false);
    if breaking {
        return Some(VersionField::Major);
    }
    match caps.name("type")?.as_str().to_ascii_lowercase().as_str() {
        "feat" => Some(VersionField::Minor),
        "fix" | "perf" | "revert" => Some(VersionField::Patch),
        _ => None,
    }
}

/// 단일 커밋 메시지에서 bump 필드 추출. 매칭 없으면 None.
fn increment_from_message(msg: &str, eff: &EffectiveConfiguration) -> Option<VersionField> {
    // Conventional Commits 모드: 규약을 우선 인식하고, 없으면 +semver 로 폴백.
    if eff.commit_message_convention == CommitMessageConvention::ConventionalCommits {
        if let Some(f) = conventional_increment(msg) {
            return Some(f);
        }
    }
    let test = |pat: &str| Regex::new(&format!("(?im){pat}")).map(|r| r.is_match(msg)).unwrap_or(false);
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

    let message_increment = if eff.commit_message_incrementing == CommitMessageIncrementMode::Disabled
    {
        None
    } else {
        let commits = ignore.filter(repo.commits_between(base_source, head_sha)?);
        let merge_only = eff.commit_message_incrementing == CommitMessageIncrementMode::MergeMessageOnly;
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

/// 메시지/브랜치명에서 버전 토큰 추출.
fn extract_version(text: &str, pattern: &str, tag_prefix: &str) -> Option<SemanticVersion> {
    let re = Regex::new(&format!("(?i){pattern}")).ok()?;
    let caps = re.captures(text)?;
    let raw = caps.name("version").map(|m| m.as_str()).unwrap_or_else(|| caps.get(0).unwrap().as_str());
    SemanticVersion::parse(raw, tag_prefix)
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
    let own = bc.increment.or(config.increment).unwrap_or(IncrementStrategy::Inherit);
    if own != IncrementStrategy::Inherit {
        return Ok(None);
    }

    let repo_branches = repo.branch_names().unwrap_or_default();
    let mut best: Option<(i64, IncrementStrategy)> = None;

    for src_key in &bc.source_branches {
        let Some(src_bc) = config.branches.get(src_key) else { continue };
        let Some(re_src) = &src_bc.regex else { continue };
        let Ok(re) = Regex::new(&format!("(?i){re_src}")) else { continue };

        // 이 source 설정에 매칭되는 실제 저장소 브랜치들.
        for rb in &repo_branches {
            if rb == branch_name {
                continue;
            }
            let short = rb.rsplit('/').next().unwrap_or(rb);
            if !(re.is_match(rb) || re.is_match(short)) {
                continue;
            }
            let Some(mb) = repo.merge_base(branch_name, rb)? else { continue };
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
        SemanticVersion::parse(sb, &eff.tag_prefix)
            .or_else(|| extract_version(sb, &eff.version_in_branch_pattern, &eff.tag_prefix))
    };

    // 사용자 정의 포맷 우선, 이어서 8종 내장 포맷.
    let custom = eff.merge_message_formats.values().map(|s| s.as_str());
    for pattern in custom.chain(BUILTIN_MERGE_FORMATS.iter().copied()) {
        let Ok(re) = Regex::new(&format!("(?s){pattern}")) else { continue };
        let Some(caps) = re.captures(message) else { continue };
        let Some(sb) = caps.name("SourceBranch") else { continue };
        let branch = sb.as_str().to_string();
        if let Some(v) = caps.name("Version").and_then(|m| SemanticVersion::parse(m.as_str(), &eff.tag_prefix)) {
            return Some((branch, v));
        }
        if let Some(v) = from_branch(&branch) {
            return Some((branch, v));
        }
        return None; // 포맷은 맞지만 버전 없음.
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
            let head = repo.commit_info_of(b).map(Ok).unwrap_or_else(|| repo.head_commit())?;
            (head, b.clone())
        }
        None => (repo.head_commit()?, repo.current_branch_name()?),
    };
    let mut eff = EffectiveConfiguration::resolve(config, &branch_name);
    let ignore = IgnoreSet::from_config(config);

    // Mainline 전략이 활성화되어 있으면 per-commit 누적 방식으로 계산.
    if config.strategies.contains(&VersionStrategy::Mainline) {
        return mainline_calculate(repo, &eff, &branch_name, &head, &ignore);
    }

    // Inherit 증분: 실제 git 조상을 따라 현재 브랜치가 분기된 source 브랜치를
    // 찾아 그 브랜치의 증분을 상속한다(설정상 첫 source 가 아니라).
    if let Some(inc) = resolve_inherit_via_git(repo, config, &branch_name)? {
        eff.increment = inc;
    }

    let mut candidates: Vec<BaseVersion> = Vec::new();
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
                if let Some(nv) = &eff.next_version {
                    if let Some(v) = parse_version(nv, &eff) {
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
            VersionStrategy::TaggedCommit | VersionStrategy::Mainline => {
                gather_tagged(repo, &eff, &head, &ignore, &mut candidates)?;
            }
            VersionStrategy::VersionInBranchName => {
                if eff.is_release_branch {
                    if let Some(v) =
                        extract_version(&branch_name, &eff.version_in_branch_pattern, &eff.tag_prefix)
                    {
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
                b.semantic_version.increment(b.increment, b.label.as_deref(), b.force_increment)
            };
            NextVersion { incremented, base: b }
        })
        .collect();

    // 최고 IncrementedVersion 후보 선택.
    let max_idx = (0..next.len())
        .max_by(|&a, &b| next[a].incremented.cmp(&next[b].incremented))
        .unwrap();

    // base version source 는 "source 를 가진 후보 중 가장 최신" 에서 가져온다
    // (원본 NextVersionCalculator 의 LatestBaseVersionSource 규칙).
    let latest_source = next
        .iter()
        .filter(|n| n.base.base_version_source.is_some())
        .max_by(|a, b| a.base.source_when.cmp(&b.base.source_when));
    let base_source = latest_source
        .and_then(|n| n.base.base_version_source.clone())
        .or_else(|| next[max_idx].base.base_version_source.clone());
    let source_semver = latest_source
        .map(|n| n.base.semantic_version.clone())
        .unwrap_or_else(|| next[max_idx].base.semantic_version.clone());

    let chosen = next.into_iter().nth(max_idx).unwrap();

    let final_semver =
        apply_deployment_mode(repo, &eff, &branch_name, &head, &chosen, base_source.as_deref(), &ignore)?;
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
            let e = tags_by_sha.entry(tag.target_sha.clone()).or_insert_with(|| core.clone());
            if core.cmp_core(e) == std::cmp::Ordering::Greater {
                *e = core;
            }
        }
    }
    let core_gt = |a: &SemanticVersion, b: &SemanticVersion| a.cmp_core(b) == std::cmp::Ordering::Greater;

    // first-parent 트렁크를 루트부터 순회. 각 step 에서 도입된 커밋의 태그가 현재
    // 버전보다 높으면 그 코어로 설정(증분 없음 = stable/pre-release 확정), 아니면 증분.
    let mut trunk = ignore.filter(repo.first_parent_between(None, &head.sha)?);
    trunk.reverse();

    let default = strategy_to_field(eff.increment);
    let mut version = SemanticVersion::new(0, 0, 0);
    let mut highest_tag = SemanticVersion::new(0, 0, 0);
    for c in &trunk {
        // 이 step 이 도입한 커밋들. merge 면 병합된 브랜치(두 번째 부모 계열), 아니면 자신.
        let introduced: Vec<CommitInfo> = if c.parents.len() >= 2 {
            ignore.filter(repo.commits_between(Some(&c.parents[0]), &c.parents[1])?)
        } else {
            vec![c.clone()]
        };

        // 도입 커밋(및 merge 커밋 자신)에 붙은 가장 높은 태그 코어.
        let mut step_tag: Option<SemanticVersion> = None;
        for sha in introduced.iter().map(|x| &x.sha).chain(std::iter::once(&c.sha)) {
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
        let mut field = default;
        for ic in &introduced {
            if let Some(f) = increment_from_message(&ic.message, eff) {
                if f > field {
                    field = f;
                }
            }
        }
        version = version.increment(field, None, true);
    }
    let base = highest_tag;

    // Mainline 의 version source 는 head 의 첫 번째 부모(직전 트렁크 상태). distance 는
    // 그로부터 head 까지의 전체 커밋 수(merge 면 병합된 커밋들 포함).
    let source_sha = head.parents.first().cloned();
    let distance = repo.commits_between(source_sha.as_deref(), &head.sha)?.len() as i64;

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

    build_variables(eff, branch_name, head, &version, &base)
}

/// HEAD 에서 도달 가능한 버전 태그를 후보로 수집.
fn gather_tagged(
    repo: &GitRepo,
    eff: &EffectiveConfiguration,
    head: &CommitInfo,
    ignore: &IgnoreSet,
    out: &mut Vec<BaseVersion>,
) -> Result<()> {
    for tag in repo.tags()? {
        if ignore.is_ignored(&tag.target_sha, &tag.when) {
            continue;
        }
        if !repo.is_ancestor_of(&tag.target_sha, &head.sha).unwrap_or(false) {
            continue;
        }
        let Some(version) = parse_version(&tag.name, &eff) else { continue };
        let is_current = tag.target_sha == head.sha;
        let exact = is_current && eff.prevent_increment_when_current_commit_tagged;
        // pre-release 태그(예: 1.0.0-beta.1)는 아직 "릴리스" 가 아니므로 버전
        // source 로 삼지 않는다. 코어를 올리지 않고, 커밋 수는 태그 커밋을
        // 포함해 그 이전부터 센다(원본 TaggedCommitVersionStrategy 동작).
        let has_pre = version.pre_release_tag.has_tag();
        let use_as_source = exact || !has_pre;
        let base_src = if use_as_source { Some(tag.target_sha.clone()) } else { None };
        let field = if exact {
            VersionField::None
        } else {
            let from = if use_as_source { Some(tag.target_sha.as_str()) } else { None };
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
    for c in ignore.filter(repo.commits_between(None, &head.sha)?) {
        let Some((merged_branch, v)) = parse_merge_message(&c.message, eff) else { continue };
        // 병합된 브랜치가 release 브랜치가 아니면 버전을 사용하지 않는다.
        if !is_release_branch(config, &merged_branch) {
            continue;
        }
        // merge 커밋의 base source 는 두 부모의 merge-base. 그래야 병합으로 들어온
        // 커밋들이 버전 소스 이후 커밋 수에 정확히 반영된다.
        let base_src = if c.parents.len() >= 2 {
            repo.merge_base(&c.parents[0], &c.parents[1])?.unwrap_or_else(|| c.sha.clone())
        } else {
            c.sha.clone()
        };
        let field = if eff.prevent_increment_of_merged_branch {
            VersionField::None
        } else {
            determine_increment(repo, Some(&base_src), &head.sha, true, eff, ignore)?
        };
        let mut bv = BaseVersion::new("MergeMessage", v, Some(base_src), field, Some(eff.label.clone()));
        bv.source_when = Some(c.when);
        out.push(bv);
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
    let Some(re_src) = &release_bc.regex else { return Ok(()) };
    let Ok(re) = Regex::new(&format!("(?i){re_src}")) else { return Ok(()) };

    for rb in repo.branch_names()? {
        let short = rb.rsplit('/').next().unwrap_or(&rb);
        if !(re.is_match(&rb) || re.is_match(short)) {
            continue;
        }
        if let Some(v) = extract_version(&rb, &eff.version_in_branch_pattern, &eff.tag_prefix) {
            let base_src = repo.merge_base(branch_name, &rb)?;
            out.push(BaseVersion::new(
                format!("TrackReleaseBranches: {rb}"),
                v,
                base_src.or(Some(head.sha.clone())),
                VersionField::None,
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
    let base_src = if chosen.base.exact { chosen.base.base_version_source.as_deref() } else { base_source };
    let commits = ignore.filter(repo.commits_between(base_src, &head.sha)?).len() as i64;
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
    let pre_tag_str = if pre.has_tag() { pre.format(false) } else { String::new() };

    let with_dash = |s: &str| if s.is_empty() { String::new() } else { format!("-{s}") };

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
    let commit_date = head.when.format(&date_fmt).to_string();

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
        version_source_increment: sv.build_metadata.version_source_increment.as_str().to_string(),
        version_source_sem_ver: source_semver.major_minor_patch(),
        version_source_sha: sv.build_metadata.version_source_sha.clone().unwrap_or_default(),
        commits_since_version_source: Some(commits),
        commit_date,
        uncommitted_changes: sv.build_metadata.uncommitted_changes,
    };

    // assembly-*-format / assembly-informational-format 커스텀 포맷 적용.
    // 포맷은 위에서 계산된 변수들을 참조하므로 여기서 후처리한다.
    let ctx = vars.to_map();
    if let Some(fmt) = &eff.assembly_versioning_format {
        vars.assembly_sem_ver = render_template(fmt, &ctx);
    }
    if let Some(fmt) = &eff.assembly_file_versioning_format {
        vars.assembly_sem_file_ver = render_template(fmt, &ctx);
    }
    // informational-format 의 기본값 `{InformationalVersion}` 은 원래 값을 그대로
    // 재현하므로 항상 적용해도 안전하다.
    vars.informational_version = render_template(&eff.assembly_informational_format, &ctx);

    Ok(vars)
}

/// `{Variable}` 및 `{env:VAR}` 토큰을 변수 맵으로 치환.
fn render_template(fmt: &str, ctx: &std::collections::BTreeMap<String, String>) -> String {
    let re = Regex::new(r"\{(?<t>[A-Za-z0-9_:]+)\}").unwrap();
    re.replace_all(fmt, |c: &regex::Captures| {
        let t = &c["t"];
        if let Some(env_var) = t.strip_prefix("env:") {
            std::env::var(env_var).unwrap_or_default()
        } else {
            ctx.get(t).cloned().unwrap_or_default()
        }
    })
    .into_owned()
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
    use super::conventional_increment;
    use crate::version::VersionField;

    #[test]
    fn conventional_types() {
        let f = |m: &str| conventional_increment(m);
        assert_eq!(f("feat: add x"), Some(VersionField::Minor));
        assert_eq!(f("feat(api): add x"), Some(VersionField::Minor));
        assert_eq!(f("fix: bug"), Some(VersionField::Patch));
        assert_eq!(f("perf: faster"), Some(VersionField::Patch));
        assert_eq!(f("revert: x"), Some(VersionField::Patch));
        // 비-릴리스 타입 → None(폴백).
        assert_eq!(f("docs: readme"), None);
        assert_eq!(f("chore: deps"), None);
        assert_eq!(f("refactor: y"), None);
        // 비규약 메시지 → None.
        assert_eq!(f("random message"), None);
        assert_eq!(f("+semver: major"), None);
    }

    #[test]
    fn conventional_breaking() {
        let f = |m: &str| conventional_increment(m);
        assert_eq!(f("feat!: drop x"), Some(VersionField::Major));
        assert_eq!(f("fix(core)!: change"), Some(VersionField::Major));
        assert_eq!(f("feat: x\n\nBREAKING CHANGE: removed"), Some(VersionField::Major));
        assert_eq!(f("feat: x\n\nBREAKING-CHANGE: removed"), Some(VersionField::Major));
        // BREAKING 푸터지만 헤더가 비규약이면 인식 안 함.
        assert_eq!(f("random\n\nBREAKING CHANGE: x"), None);
    }
}
