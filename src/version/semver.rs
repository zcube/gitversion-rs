//! SemanticVersion data model.
//!
//! Corresponds to the upstream `GitVersion.Core/SemVer/SemanticVersion.cs`,
//! `SemanticVersionPreReleaseTag.cs`, and `SemanticVersionBuildMetaData.cs`.

use chrono::{DateTime, FixedOffset};
use std::cmp::Ordering;
use std::fmt;

use super::VersionField;

/// Pre-release tag. Example: `beta.1` => name="beta", number=Some(1).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PreReleaseTag {
    pub name: String,
    pub number: Option<i64>,
    /// Even if the name is empty, treat as a tag (promote) when a number is present.
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

    /// Returns true if a meaningful pre-release tag exists.
    pub fn has_tag(&self) -> bool {
        !self.name.is_empty() || (self.number.is_some() && self.promote_tag_even_if_name_is_empty)
    }

    /// Corresponds to the upstream `SemanticVersionPreReleaseTag.Parse`. Supports `beta.1`, `beta`, and `1` forms.
    ///
    /// Sets `promote_tag_even_if_name_is_empty = true` for non-empty input,
    /// so a number-only pre-release (e.g. `1`, where `name=""`) correctly returns `has_tag() = true`.
    pub fn parse(input: &str) -> Self {
        if input.trim().is_empty() {
            return Self::default();
        }
        // Split name and trailing number: the trailing digits (optionally preceded by '.') become the number.
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

    /// `t` format: name only. Default format: `name.number`.
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
        // No tag (stable) > has tag (pre-release)
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

/// Build metadata: commits-since-tag, branch, sha, etc.
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
    /// Keep only safe characters: `[^0-9A-Za-z-.]` => `-`.
    /// The Branch part of InformationalVersion allows dots.
    /// The upstream .NET GitVersion BuildMetaData sanitizes branches by replacing
    /// special characters like slashes but preserving dots.
    fn sanitize(s: &str) -> String {
        let re = regex::Regex::new(r"[^0-9A-Za-z\-.]").unwrap();
        re.replace_all(s, "-").into_owned()
    }

    /// Short format `b`: commits-since-tag only.
    pub fn format_short(&self) -> String {
        self.commits_since_tag
            .map(|c| c.to_string())
            .unwrap_or_default()
    }

    /// Full format `f`: commits.Branch.<branch>.Sha.<sha>[.other].
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

/// A complete semantic version.
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

    /// Returns only `Major.Minor.Patch`.
    pub fn major_minor_patch(&self) -> String {
        format!("{}.{}.{}", self.major, self.minor, self.patch)
    }

    /// Parse a version string (Loose). Strips the leading prefix matched by `tag_prefix` before parsing.
    /// Examples: `v1.2.3-beta.4`, `1.2`, `1`.
    pub fn parse(input: &str, tag_prefix: &str) -> Option<Self> {
        Self::parse_with(input, tag_prefix, false)
    }

    /// Parse a version string. When `strict` is true, all three components (Major.Minor.Patch) are
    /// required, as in SemVer 2.0 (mirrors the original `SemanticVersionFormat.Strict`). Loose allows partial versions.
    pub fn parse_with(input: &str, tag_prefix: &str, strict: bool) -> Option<Self> {
        let trimmed = input.trim();
        // Strip the tag prefix.
        let body = if tag_prefix.is_empty() {
            trimmed.to_string()
        } else {
            let re = regex::Regex::new(&format!("^({})", tag_prefix)).ok()?;
            re.replace(trimmed, "").into_owned()
        };
        let body = body.trim();
        if strict {
            Self::parse_strict(body)
        } else {
            Self::parse_loose(body)
        }
    }

    /// Mirrors the original `ParseStrictRegex`: all three of major.minor.patch required, no leading zeros
    /// (`0|[1-9]\d*`), SemVer 2.0 pre-release/build metadata.
    fn parse_strict(body: &str) -> Option<Self> {
        let re = regex::Regex::new(
            r"^(?<major>0|[1-9]\d*)\.(?<minor>0|[1-9]\d*)\.(?<patch>0|[1-9]\d*)(?:-(?<tag>(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?(?:\+(?<meta>[0-9a-zA-Z-]+(?:\.[0-9a-zA-Z-]+)*))?$",
        )
        .ok()?;
        let c = re.captures(body)?;
        Some(Self {
            major: c.name("major")?.as_str().parse().ok()?,
            minor: c.name("minor")?.as_str().parse().ok()?,
            patch: c.name("patch")?.as_str().parse().ok()?,
            pre_release_tag: c
                .name("tag")
                .map(|m| PreReleaseTag::parse(m.as_str()))
                .unwrap_or_default(),
            build_metadata: BuildMetaData::default(),
        })
    }

    /// Mirrors the original `ParseLooseRegex`: minor/patch are optional, leading zeros allowed (`\d+`),
    /// the fourth numeric part (FourthPart) is interpreted as commits-since-tag, and
    /// pre-release tags are accepted loosely up to `+`.
    fn parse_loose(body: &str) -> Option<Self> {
        let re = regex::Regex::new(
            r"^(?<major>\d+)(\.(?<minor>\d+))?(\.(?<patch>\d+))?(\.(?<fourth>\d+))?(-(?<tag>[^+]*))?(\+(?<meta>.*))?$",
        )
        .ok()?;
        let c = re.captures(body)?;
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
        let build_metadata = BuildMetaData {
            commits_since_tag: c.name("fourth").and_then(|m| m.as_str().parse().ok()),
            ..Default::default()
        };
        Some(Self {
            major,
            minor,
            patch,
            pre_release_tag,
            build_metadata,
        })
    }

    /// Compare only the core version (ignoring pre-release).
    pub fn cmp_core(&self, other: &Self) -> Ordering {
        self.major
            .cmp(&other.major)
            .then(self.minor.cmp(&other.minor))
            .then(self.patch.cmp(&other.patch))
    }

    /// Increment the specified field and apply the label. Mirrors the original `SemanticVersion.Increment`.
    ///
    /// - If a pre-release already exists and `force` is false, the core is not bumped; the pre-release is kept.
    /// - `label` of `Some("")` (empty string) creates a name-less promoted pre-release that still
    ///   exposes its number (e.g. `0.0.1-1`). `None` means no label is applied.
    pub fn increment(&self, field: VersionField, label: Option<&str>, force: bool) -> Self {
        let mut v = self.clone();
        let has_pre = self.pre_release_tag.has_tag();
        // Do not bump the core if a pre-release already exists (unless forced).
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

        // Bumping the core resets any existing pre-release.
        if bump_core && field != VersionField::None {
            v.pre_release_tag = PreReleaseTag::default();
        }

        // Apply the label.
        if let Some(l) = label {
            if v.pre_release_tag.has_tag() && v.pre_release_tag.name == l {
                // Same label: bump the number only.
                v.pre_release_tag.number = Some(v.pre_release_tag.number.unwrap_or(0) + 1);
            } else {
                // New label. When the name is empty, promote to expose the number.
                v.pre_release_tag = PreReleaseTag::new(l, Some(1), l.is_empty());
            }
        }
        v
    }
}

impl fmt::Display for SemanticVersion {
    /// `s` format: Major.Minor.Patch[-pre].
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
        // Empty label → name-less promoted pre-release (e.g. 0.0.1-1).
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
        // Pre-release already exists: keep the core, bump the number only.
        assert_eq!(v.to_string(), "1.1.0-alpha.2");
    }

    #[test]
    fn strict_rejects_partial_version() {
        // Strict requires all three of Major.Minor.Patch.
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
    fn strict_rejects_four_part_and_leading_zero() {
        // Strict (original ParseStrictRegex): rejects 4-part and leading zeros.
        assert!(SemanticVersion::parse_with("1.2.3.4", "[vV]?", true).is_none());
        assert!(SemanticVersion::parse_with("01.02.03", "[vV]?", true).is_none());
        assert!(SemanticVersion::parse_with("1.2.3", "[vV]?", true).is_some());
    }

    #[test]
    fn loose_accepts_four_part_and_leading_zero() {
        // Loose (original ParseLooseRegex): the fourth part is interpreted as commits-since-tag.
        let v = SemanticVersion::parse_with("1.2.3.4", "[vV]?", false).unwrap();
        assert_eq!((v.major, v.minor, v.patch), (1, 2, 3));
        assert_eq!(v.build_metadata.commits_since_tag, Some(4));
        // Leading zeros are allowed: 01.02.03 → 1.2.3.
        let v = SemanticVersion::parse_with("01.02.03", "[vV]?", false).unwrap();
        assert_eq!((v.major, v.minor, v.patch), (1, 2, 3));
        // 3-part versions have no commits-since-tag.
        let v = SemanticVersion::parse_with("1.2.3", "[vV]?", false).unwrap();
        assert_eq!(v.build_metadata.commits_since_tag, None);
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
        // name is empty and only number is set → returns the number as a string.
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
        // Neither has a tag → Equal.
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
        // Empty other_metadata must not appear in the output.
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
