//! 버전 데이터 모델과 계산 엔진.

pub mod calculation;
pub mod semver;

pub use semver::{BuildMetaData, PreReleaseTag, SemanticVersion};

/// 증분 대상 필드. 원본 `GitVersion.Core/SemVer/VersionField.cs`.
///
/// 정렬 순서가 곧 우선순위: `None < Patch < Minor < Major`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum VersionField {
    #[default]
    None,
    Patch,
    Minor,
    Major,
}

impl VersionField {
    pub fn as_str(&self) -> &'static str {
        match self {
            VersionField::None => "None",
            VersionField::Patch => "Patch",
            VersionField::Minor => "Minor",
            VersionField::Major => "Major",
        }
    }
}
