//! 버전 계산 결과 디스크 캐시.
//!
//! 원본 `GitVersion.Core/VersionCalculation/Caching` 대응. 저장소 상태(refs +
//! HEAD), 설정 파일 내용, overrideconfig 값을 SHA1 으로 해시한 키로 결과를
//! `<.git>/gitversion_cache/<키>.json` 에 저장하고 재사용한다. 저장소 상태나 설정이
//! 바뀌면 키가 달라져 자동으로 무효화된다.

use crate::git::GitRepo;
use crate::output::VersionVariables;
use sha1::{Digest, Sha1};
use std::path::{Path, PathBuf};

/// 캐시 키 계산: (refs 스냅샷 + HEAD + 설정파일 내용 + overrideconfig)의 SHA1 hex.
pub fn compute_key(repo: &GitRepo, config_path: Option<&Path>, overrides: &[String]) -> String {
    let mut hasher = Sha1::new();

    // 1) refs 스냅샷(모든 브랜치/태그의 이름+target).
    for line in repo.refs_snapshot().unwrap_or_default() {
        hasher.update(line.as_bytes());
        hasher.update(b"\n");
    }
    hasher.update(b"--head--");
    // 2) HEAD ref 이름 + tip sha.
    hasher.update(repo.head_ref_name().as_bytes());
    if let Ok(head) = repo.head_commit() {
        hasher.update(head.sha.as_bytes());
    }
    hasher.update(b"--config--");
    // 3) 설정 파일 내용.
    if let Some(p) = config_path {
        if let Ok(content) = std::fs::read_to_string(p) {
            hasher.update(content.as_bytes());
        }
    }
    hasher.update(b"--override--");
    // 4) overrideconfig 값.
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

/// 캐시 파일 경로: `<.git>/gitversion_cache/<키>.json`.
fn cache_file(repo: &GitRepo, key: &str) -> PathBuf {
    repo.git_dir().join("gitversion_cache").join(format!("{key}.json"))
}

/// 캐시에서 변수 로드. 없거나 손상되면 None(손상 시 삭제).
pub fn load(repo: &GitRepo, key: &str) -> Option<VersionVariables> {
    let path = cache_file(repo, key);
    if !path.is_file() {
        log::debug!("캐시 미스: {}", path.display());
        return None;
    }
    match std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok()) {
        Some(vars) => {
            log::info!("캐시 적중: {}", path.display());
            Some(vars)
        }
        None => {
            log::warn!("캐시 파일이 손상되어 삭제합니다: {}", path.display());
            let _ = std::fs::remove_file(&path);
            None
        }
    }
}

/// 변수를 캐시에 기록.
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
                log::warn!("캐시 기록 실패 {}: {e}", path.display());
            } else {
                log::debug!("캐시 기록: {}", path.display());
            }
        }
        Err(e) => log::warn!("캐시 직렬화 실패: {e}"),
    }
}
