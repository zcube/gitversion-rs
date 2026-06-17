//! SemanticVersion 데이터 모델.
//!
//! 원본 `GitVersion.Core/SemVer/SemanticVersion.cs`,
//! `SemanticVersionPreReleaseTag.cs`, `SemanticVersionBuildMetaData.cs` 대응.

use chrono::{DateTime, FixedOffset};
use std::cmp::Ordering;
use std::fmt;

use super::VersionField;

/// pre-release 태그. 예: `beta.1` => name="beta", number=Some(1).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PreReleaseTag {
    pub name: String,
    pub number: Option<i64>,
    /// 이름이 비어 있어도 number 가 있으면 태그로 취급(promote).
    pub promote_tag_even_if_name_is_empty: bool,
}

impl PreReleaseTag {
    pub fn new(name: impl Into<String>, number: Option<i64>, promote: bool) -> Self {
        Self {
            name: name.into(),
            number,
            promote_tag_even_if_name_is_empty: promote,
        }
    }

    /// 의미 있는 태그가 존재하는지.
    pub fn has_tag(&self) -> bool {
        !self.name.is_empty() || (self.number.is_some() && self.promote_tag_even_if_name_is_empty)
    }

    /// 원본 `SemanticVersionPreReleaseTag.Parse`. `beta.1`, `beta`, `1` 형태 지원.
    ///
    /// 입력이 비어있지 않으면 `promote_tag_even_if_name_is_empty = true` 로 설정한다.
    /// 이로 인해 `1` 처럼 숫자만 있는 pre-release(`name=""`)도 `has_tag() = true` 가 되어
    /// pre-release 태그로 올바르게 인식된다.
    pub fn parse(input: &str) -> Self {
        if input.trim().is_empty() {
            return Self::default();
        }
        // name 과 trailing number 분리: 끝의 숫자(앞에 '.' 또는 없이)를 number 로.
        let re = regex::Regex::new(r"(?<name>.*?)\.?(?<number>\d+)?$").unwrap();
        if let Some(c) = re.captures(input) {
            let name = c.name("name").map(|m| m.as_str()).unwrap_or("").to_string();
            let number = c
                .name("number")
                .and_then(|m| m.as_str().parse::<i64>().ok());
            return Self {
                name,
                number,
                promote_tag_even_if_name_is_empty: true,
            };
        }
        Self {
            name: input.to_string(),
            number: None,
            promote_tag_even_if_name_is_empty: true,
        }
    }

    /// `t` 포맷: 이름만. 기본 포맷: `name.number`.
    pub fn format(&self, legacy_dash: bool) -> String {
        let _ = legacy_dash;
        match self.number {
            Some(n) if !self.name.is_empty() => format!("{}.{}", self.name, n),
            Some(n) => n.to_string(),
            None => self.name.clone(),
        }
    }
}

impl Ord for PreReleaseTag {
    fn cmp(&self, other: &Self) -> Ordering {
        // 태그 없음(안정 버전) > 태그 있음(pre-release)
        match (self.has_tag(), other.has_tag()) {
            (false, false) => Ordering::Equal,
            (false, true) => Ordering::Greater,
            (true, false) => Ordering::Less,
            (true, true) => self
                .name
                .to_lowercase()
                .cmp(&other.name.to_lowercase())
                .then(self.number.unwrap_or(-1).cmp(&other.number.unwrap_or(-1))),
        }
    }
}
impl PartialOrd for PreReleaseTag {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// build metadata. commits-since-tag, branch, sha 등.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuildMetaData {
    pub commits_since_tag: Option<i64>,
    pub branch: Option<String>,
    pub sha: Option<String>,
    pub short_sha: Option<String>,
    pub commit_date: Option<DateTime<FixedOffset>>,
    pub other_metadata: Option<String>,
    pub version_source_sha: Option<String>,
    pub version_source_distance: i64,
    pub uncommitted_changes: i64,
    pub version_source_increment: VersionField,
}

impl BuildMetaData {
    /// 안전 문자만 남기기: `[^0-9A-Za-z-.]` => `-`.
    /// InformationalVersion 의 Branch 부분은 점(.)을 허용한다.
    /// 원본 .NET GitVersion 의 BuildMetaData 에서 branch 를 sanitize 할 때
    /// 슬래시 등 특수문자는 치환하지만 점은 유지한다.
    fn sanitize(s: &str) -> String {
        let re = regex::Regex::new(r"[^0-9A-Za-z\-.]").unwrap();
        re.replace_all(s, "-").into_owned()
    }

    /// 기본 포맷 `b`: commits-since-tag 만.
    pub fn format_short(&self) -> String {
        self.commits_since_tag
            .map(|c| c.to_string())
            .unwrap_or_default()
    }

    /// 완전 포맷 `f`: commits.Branch.<branch>.Sha.<sha>[.other].
    pub fn format_full(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(c) = self.commits_since_tag {
            parts.push(c.to_string());
        }
        if let Some(b) = &self.branch {
            parts.push(format!("Branch.{}", Self::sanitize(b)));
        }
        if let Some(s) = &self.sha {
            parts.push(format!("Sha.{}", s));
        }
        if let Some(o) = &self.other_metadata {
            if !o.is_empty() {
                parts.push(Self::sanitize(o));
            }
        }
        parts.join(".")
    }
}

/// 완전한 의미론적 버전.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SemanticVersion {
    pub major: i64,
    pub minor: i64,
    pub patch: i64,
    pub pre_release_tag: PreReleaseTag,
    pub build_metadata: BuildMetaData,
}

impl SemanticVersion {
    pub fn new(major: i64, minor: i64, patch: i64) -> Self {
        Self {
            major,
            minor,
            patch,
            ..Default::default()
        }
    }

    /// `Major.Minor.Patch` 만.
    pub fn major_minor_patch(&self) -> String {
        format!("{}.{}.{}", self.major, self.minor, self.patch)
    }

    /// 버전 문자열 파싱(Loose). `tag_prefix` 정규식으로 접두어 제거 후 파싱.
    /// 예: `v1.2.3-beta.4`, `1.2`, `1`.
    pub fn parse(input: &str, tag_prefix: &str) -> Option<Self> {
        Self::parse_with(input, tag_prefix, false)
    }

    /// 버전 문자열 파싱. `strict` 이면 SemVer 2.0 처럼 Major.Minor.Patch 3요소를
    /// 모두 요구한다(원본 `SemanticVersionFormat.Strict`). Loose 면 부분 버전 허용.
    pub fn parse_with(input: &str, tag_prefix: &str, strict: bool) -> Option<Self> {
        let trimmed = input.trim();
        // tag prefix 제거
        let body = if tag_prefix.is_empty() {
            trimmed.to_string()
        } else {
            let re = regex::Regex::new(&format!("^({})", tag_prefix)).ok()?;
            re.replace(trimmed, "").into_owned()
        };
        // 주의: 원본 ParseLooseRegex 에는 4번째 숫자 파트(FourthPart)가 있으나,
        // 실제 태그/버전 파싱 경로(SemanticVersionFormat=Strict 가 기본)에서는
        // `1.2.3.4` 같은 4-part 가 버전으로 인식되지 않는다(태그 무시 → fallback).
        // 따라서 여기서도 4-part 는 매칭하지 않아 원본과 동작을 맞춘다.
        let re = regex::Regex::new(
            r"^(?<major>\d+)(\.(?<minor>\d+))?(\.(?<patch>\d+))?(-(?<tag>[0-9A-Za-z\-.]+))?(\+(?<meta>[0-9A-Za-z\-.]+))?$",
        )
        .ok()?;
        let c = re.captures(body.trim())?;
        if strict && (c.name("minor").is_none() || c.name("patch").is_none()) {
            return None;
        }
        let major = c.name("major")?.as_str().parse().ok()?;
        let minor = c
            .name("minor")
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(0);
        let patch = c
            .name("patch")
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(0);
        let pre_release_tag = c
            .name("tag")
            .map(|m| PreReleaseTag::parse(m.as_str()))
            .unwrap_or_default();
        Some(Self {
            major,
            minor,
            patch,
            pre_release_tag,
            build_metadata: BuildMetaData::default(),
        })
    }

    /// 코어 버전만 비교(pre-release 무시).
    pub fn cmp_core(&self, other: &Self) -> Ordering {
        self.major
            .cmp(&other.major)
            .then(self.minor.cmp(&other.minor))
            .then(self.patch.cmp(&other.patch))
    }

    /// 지정 필드를 증분하고 label 을 적용. 원본 `SemanticVersion.Increment`.
    ///
    /// - 이미 pre-release 가 있고 force 가 아니면 코어 대신 pre-release 를 유지한다.
    /// - `label` 이 `Some("")`(빈 문자열)이면 이름 없는, 그러나 번호를 노출하는
    ///   promoted pre-release 를 만든다(예: `0.0.1-1`). `None` 이면 label 미적용.
    pub fn increment(&self, field: VersionField, label: Option<&str>, force: bool) -> Self {
        let mut v = self.clone();
        let has_pre = self.pre_release_tag.has_tag();
        // 이미 pre-release 가 있으면(그리고 force 아님) 코어를 올리지 않는다.
        let bump_core = !has_pre || force;

        match field {
            VersionField::None => {}
            VersionField::Patch if bump_core => v.patch += 1,
            VersionField::Minor if bump_core => {
                v.minor += 1;
                v.patch = 0;
            }
            VersionField::Major if bump_core => {
                v.major += 1;
                v.minor = 0;
                v.patch = 0;
            }
            _ => {}
        }

        // 코어를 실제로 올렸다면 기존 pre-release 는 초기화된다.
        if bump_core && field != VersionField::None {
            v.pre_release_tag = PreReleaseTag::default();
        }

        // label 적용.
        if let Some(l) = label {
            if v.pre_release_tag.has_tag() && v.pre_release_tag.name == l {
                // 같은 label 이면 번호만 증가.
                v.pre_release_tag.number = Some(v.pre_release_tag.number.unwrap_or(0) + 1);
            } else {
                // 새 label. 이름이 비어 있으면 번호를 노출하기 위해 promote.
                v.pre_release_tag = PreReleaseTag::new(l, Some(1), l.is_empty());
            }
        }
        v
    }
}

impl fmt::Display for SemanticVersion {
    /// `s` 포맷: Major.Minor.Patch[-pre].
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.major_minor_patch())?;
        if self.pre_release_tag.has_tag() {
            write!(f, "-{}", self.pre_release_tag.format(false))?;
        }
        Ok(())
    }
}

impl Ord for SemanticVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        self.cmp_core(other)
            .then(self.pre_release_tag.cmp(&other.pre_release_tag))
    }
}
impl PartialOrd for SemanticVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::version::VersionField;

    #[test]
    fn parse_basic() {
        let v = SemanticVersion::parse("v1.2.3", "[vV]?").unwrap();
        assert_eq!((v.major, v.minor, v.patch), (1, 2, 3));
        assert!(!v.pre_release_tag.has_tag());
    }

    #[test]
    fn parse_partial_and_prerelease() {
        let v = SemanticVersion::parse("1.2", "[vV]?").unwrap();
        assert_eq!((v.major, v.minor, v.patch), (1, 2, 0));
        let v = SemanticVersion::parse("2.0.0-beta.4", "[vV]?").unwrap();
        assert_eq!(v.pre_release_tag.name, "beta");
        assert_eq!(v.pre_release_tag.number, Some(4));
    }

    #[test]
    fn ordering_stable_gt_prerelease() {
        let stable = SemanticVersion::parse("1.0.0", "").unwrap();
        let pre = SemanticVersion::parse("1.0.0-alpha.1", "").unwrap();
        assert!(stable > pre);
    }

    #[test]
    fn increment_empty_label_promotes_number() {
        // 빈 label → 이름 없는 promoted pre-release (예: 0.0.1-1).
        let base = SemanticVersion::new(0, 0, 0);
        let v = base.increment(VersionField::Patch, Some(""), false);
        assert_eq!(v.major_minor_patch(), "0.0.1");
        assert_eq!(v.to_string(), "0.0.1-1");
        assert_eq!(v.pre_release_tag.number, Some(1));
    }

    #[test]
    fn increment_named_label_resets_to_one() {
        let base = SemanticVersion::new(1, 0, 0);
        let v = base.increment(VersionField::Minor, Some("alpha"), false);
        assert_eq!(v.to_string(), "1.1.0-alpha.1");
    }

    #[test]
    fn increment_same_label_bumps_number() {
        let mut base = SemanticVersion::new(1, 1, 0);
        base.pre_release_tag = PreReleaseTag::new("alpha", Some(1), false);
        let v = base.increment(VersionField::Minor, Some("alpha"), false);
        // 이미 pre-release 가 있으므로 코어 유지, 번호만 증가.
        assert_eq!(v.to_string(), "1.1.0-alpha.2");
    }

    #[test]
    fn strict_rejects_partial_version() {
        // Strict 는 Major.Minor.Patch 3요소를 모두 요구.
        assert!(SemanticVersion::parse_with("1.2", "[vV]?", true).is_none());
        assert!(SemanticVersion::parse_with("1", "[vV]?", true).is_none());
        assert!(SemanticVersion::parse_with("1.2.3", "[vV]?", true).is_some());
    }

    #[test]
    fn loose_accepts_partial_version() {
        let v = SemanticVersion::parse_with("1.2", "[vV]?", false).unwrap();
        assert_eq!((v.major, v.minor, v.patch), (1, 2, 0));
        let v = SemanticVersion::parse_with("v1", "[vV]?", false).unwrap();
        assert_eq!((v.major, v.minor, v.patch), (1, 0, 0));
    }

    #[test]
    fn loose_rejects_four_part_version() {
        // 원본 GitVersion 의 태그 파싱(SemanticVersionFormat=Strict 기본)은 `1.2.3.4`
        // 같은 4-part 를 버전으로 인식하지 않는다. 우리도 거부해 동작을 맞춘다.
        assert!(SemanticVersion::parse_with("1.2.3.4", "[vV]?", false).is_none());
        assert!(SemanticVersion::parse_with("v1.2.3.4", "[vV]?", false).is_none());
        // 3-part 는 정상 파싱된다.
        assert!(SemanticVersion::parse_with("1.2.3", "[vV]?", false).is_some());
    }

    #[test]
    fn increment_none_keeps_core() {
        let base = SemanticVersion::new(2, 0, 0);
        let v = base.increment(VersionField::None, Some(""), false);
        assert_eq!(v.major_minor_patch(), "2.0.0");
        assert_eq!(v.to_string(), "2.0.0-1");
    }

    #[test]
    fn prerelease_tag_parse_empty_returns_default() {
        let t = PreReleaseTag::parse("");
        assert!(!t.has_tag());
        assert_eq!(t.name, "");
        assert_eq!(t.number, None);
    }

    #[test]
    fn prerelease_tag_format_number_only() {
        // name 이 비어 있고 number 만 있는 경우 → 숫자 문자열 반환.
        let t = PreReleaseTag::new("", Some(3), true);
        assert_eq!(t.format(false), "3");
    }

    #[test]
    fn prerelease_tag_format_name_and_number() {
        let t = PreReleaseTag::new("rc", Some(2), false);
        assert_eq!(t.format(false), "rc.2");
    }

    #[test]
    fn prerelease_tag_ordering_both_without_tag() {
        // 둘 다 태그 없음 → Equal.
        let a = PreReleaseTag::default();
        let b = PreReleaseTag::default();
        assert_eq!(a.cmp(&b), std::cmp::Ordering::Equal);
        assert_eq!(a.partial_cmp(&b), Some(std::cmp::Ordering::Equal));
    }

    #[test]
    fn prerelease_tag_ordering_with_vs_without() {
        let stable = PreReleaseTag::default();
        let pre = PreReleaseTag::new("alpha", Some(1), false);
        assert!(stable > pre);
        assert!(pre < stable);
    }

    #[test]
    fn build_metadata_format_short_none() {
        let meta = BuildMetaData::default();
        assert_eq!(meta.format_short(), "");
    }

    #[test]
    fn build_metadata_format_short_value() {
        let meta = BuildMetaData {
            commits_since_tag: Some(5),
            ..Default::default()
        };
        assert_eq!(meta.format_short(), "5");
    }

    #[test]
    fn build_metadata_format_full_all_fields() {
        let meta = BuildMetaData {
            commits_since_tag: Some(3),
            branch: Some("feature/foo".into()),
            sha: Some("abc1234".into()),
            other_metadata: Some("extra!info".into()),
            ..Default::default()
        };
        let full = meta.format_full();
        assert!(full.contains("3"), "commits: {full}");
        assert!(
            full.contains("Branch.feature-foo"),
            "branch sanitize: {full}"
        );
        assert!(full.contains("Sha.abc1234"), "sha: {full}");
        assert!(full.contains("extra-info"), "other sanitize: {full}");
    }

    #[test]
    fn build_metadata_format_full_empty_other_omitted() {
        let meta = BuildMetaData {
            commits_since_tag: Some(1),
            other_metadata: Some(String::new()),
            ..Default::default()
        };
        let full = meta.format_full();
        // 빈 other_metadata 는 출력에 포함되지 않아야 함.
        assert_eq!(full, "1");
    }

    #[test]
    fn semver_display_no_prerelease() {
        let v = SemanticVersion::new(1, 2, 3);
        assert_eq!(v.to_string(), "1.2.3");
    }

    #[test]
    fn semver_partial_ord() {
        let a = SemanticVersion::new(1, 0, 0);
        let b = SemanticVersion::new(2, 0, 0);
        assert!(a < b);
        assert!(a.partial_cmp(&b) == Some(std::cmp::Ordering::Less));
    }

    #[test]
    fn increment_major_resets_minor_patch() {
        let base = SemanticVersion::new(1, 2, 3);
        let v = base.increment(VersionField::Major, None, true);
        assert_eq!((v.major, v.minor, v.patch), (2, 0, 0));
    }
}
