//! Disk cache for version calculation results.
//!
//! Corresponds to `GitVersion.Core/VersionCalculation/Caching`. Caches results
//! under `<.git>/gitversion_cache/<key>.json`, keyed by a SHA1 hash of the
//! repository state (refs + HEAD), config file content, and overrideconfig values.
//! The cache is automatically invalidated when the repository state or config changes.

use crate::git::GitRepo;
use crate::output::VersionVariables;
use rust_i18n::t;
use sha1::{Digest, Sha1};
use std::path::{Path, PathBuf};

/// Compute cache key: SHA1 hex of (refs snapshot + HEAD + config file content + overrideconfig).
///
/// Corresponds to the four components of `GitVersionCacheKeyFactory`
/// (gitSystemHash, repositorySnapshotHash, configFileHash, overrideConfigHash).
/// The GitVersion binary version is intentionally **not** included in the key,
/// so the cache is reused as long as the repository and config are unchanged
/// (use `--no-cache` during development).
pub fn compute_key(repo: &GitRepo, config_path: Option<&Path>, overrides: &[String]) -> String {
    let mut hasher = Sha1::new();

    // 1) refs snapshot (name + target for all branches/tags).
    for line in repo.refs_snapshot().unwrap_or_default() {
        hasher.update(line.as_bytes());
        hasher.update(b"\n");
    }
    hasher.update(b"--head--");
    // 2) HEAD ref name + tip SHA.
    hasher.update(repo.head_ref_name().as_bytes());
    if let Ok(head) = repo.head_commit() {
        hasher.update(head.sha.as_bytes());
    }
    hasher.update(b"--config--");
    // 3) Config file content.
    if let Some(p) = config_path {
        if let Ok(content) = std::fs::read_to_string(p) {
            hasher.update(content.as_bytes());
        }
    }
    hasher.update(b"--override--");
    // 4) overrideconfig values.
    for o in overrides {
        hasher.update(o.as_bytes());
        hasher.update(b"\n");
    }

    let digest = hasher.finalize();
    let mut hex = String::with_capacity(40);
    for b in digest {
        hex.push_str(&format!("{b:02x}"));
    }
    hex
}

/// Path to the cache file: `<.git>/gitversion_cache/<key>.json`.
fn cache_file(repo: &GitRepo, key: &str) -> PathBuf {
    repo.git_dir()
        .join("gitversion_cache")
        .join(format!("{key}.json"))
}

/// Load variables from cache. Returns None if missing or corrupt (deletes corrupt entries).
pub fn load(repo: &GitRepo, key: &str) -> Option<VersionVariables> {
    let path = cache_file(repo, key);
    if !path.is_file() {
        log::debug!("cache miss: {}", path.display());
        return None;
    }
    match std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
    {
        Some(vars) => {
            log::debug!("{}", t!("cache.hit", path = path.display()));
            Some(vars)
        }
        None => {
            log::warn!("{}", t!("cache.corrupt", path = path.display()));
            let _ = std::fs::remove_file(&path);
            None
        }
    }
}

/// Write variables to the cache.
pub fn store(repo: &GitRepo, key: &str, vars: &VersionVariables) {
    let path = cache_file(repo, key);
    if let Some(parent) = path.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    match serde_json::to_string_pretty(vars) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                log::warn!(
                    "{}",
                    t!("cache.write_failed", path = path.display(), error = e)
                );
            } else {
                log::debug!("cache write: {}", path.display());
            }
        }
        Err(e) => log::warn!("{}", t!("cache.serialize_failed", error = e)),
    }
}
