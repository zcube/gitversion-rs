//! Version data model and calculation engine.

pub mod calculation;
pub mod semver;

pub use semver::{BuildMetaData, PreReleaseTag, SemanticVersion};

/// Version field to increment. Corresponds to `GitVersion.Core/SemVer/VersionField.cs`.
///
/// Sort order is also priority: `None < Patch < Minor < Major`.
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
