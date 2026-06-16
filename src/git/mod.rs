//! gix(gitoxide) 기반 순수 Rust 저장소 접근 계층.
//!
//! 원본 `GitVersion.LibGit2Sharp` 에 대응하며, 버전 계산에 필요한
//! 최소 그래프 연산(태그 수집, 커밋 워킹, merge-base, 미커밋 변경)을 제공한다.

use anyhow::{Context, Result};
use chrono::{DateTime, FixedOffset, TimeZone};
use gix::ObjectId;
use rust_i18n::t;
use std::collections::HashSet;
use std::path::Path;

/// 단일 커밋 요약.
#[derive(Debug, Clone)]
pub struct CommitInfo {
    pub sha: String,
    pub short_sha: String,
    pub message: String,
    pub when: DateTime<FixedOffset>,
    pub parent_count: usize,
    /// 부모 커밋 SHA 목록.
    pub parents: Vec<String>,
}

/// 버전 태그 후보.
#[derive(Debug, Clone)]
pub struct TagInfo {
    pub name: String,
    /// 태그가 가리키는 커밋 SHA(annotated 태그는 peel 후).
    pub target_sha: String,
    pub when: DateTime<FixedOffset>,
}

/// 저장소 래퍼.
pub struct GitRepo {
    repo: gix::Repository,
}

fn gix_time_to_chrono(t: gix::date::Time) -> DateTime<FixedOffset> {
    let offset =
        FixedOffset::east_opt(t.offset).unwrap_or_else(|| FixedOffset::east_opt(0).unwrap());
    offset
        .timestamp_opt(t.seconds, 0)
        .single()
        .unwrap_or_else(|| offset.timestamp_opt(0, 0).unwrap())
}

impl GitRepo {
    /// `path` 또는 상위에서 `.git` 을 탐색해 연다.
    pub fn discover(path: &Path) -> Result<Self> {
        let repo =
            gix::discover(path).with_context(|| t!("git.repo_not_found", path = path.display()))?;
        Ok(Self { repo })
    }

    /// 저장소 작업 트리 루트.
    pub fn workdir(&self) -> Option<&Path> {
        self.repo.workdir()
    }

    /// `.git` 디렉터리 경로(캐시 위치 계산용).
    pub fn git_dir(&self) -> &Path {
        self.repo.git_dir()
    }

    /// HEAD 의 canonical ref 이름(detached 면 short sha).
    pub fn head_ref_name(&self) -> String {
        match self.repo.head_name() {
            Ok(Some(name)) => name.as_bstr().to_string(),
            _ => self
                .head_commit()
                .map(|c| c.short_sha)
                .unwrap_or_else(|_| "HEAD".into()),
        }
    }

    /// 모든 ref 의 "이름 target_sha" 목록(정렬). 캐시 키의 refs 스냅샷용.
    pub fn refs_snapshot(&self) -> Result<Vec<String>> {
        let mut out = Vec::new();
        if let Ok(platform) = self.repo.references() {
            if let Ok(iter) = platform.all() {
                for reference in iter.flatten() {
                    let name = reference.name().as_bstr().to_string();
                    let target = reference
                        .clone()
                        .into_fully_peeled_id()
                        .map(|id| id.to_string())
                        .unwrap_or_default();
                    out.push(format!("{name} {target}"));
                }
            }
        }
        out.sort();
        Ok(out)
    }

    fn commit_info(commit: &gix::Commit<'_>) -> Result<CommitInfo> {
        let sha = commit.id().to_string();
        let when = gix_time_to_chrono(commit.time()?);
        let message = commit
            .message_raw()
            .map(|m| m.to_string())
            .unwrap_or_default();
        let parents: Vec<String> = commit.parent_ids().map(|id| id.to_string()).collect();
        Ok(CommitInfo {
            short_sha: sha[..7.min(sha.len())].to_string(),
            sha,
            message,
            when,
            parent_count: parents.len(),
            parents,
        })
    }

    /// HEAD 가 가리키는 커밋.
    pub fn head_commit(&self) -> Result<CommitInfo> {
        let commit = self
            .repo
            .head_commit()
            .with_context(|| t!("git.head_read").to_string())?;
        Self::commit_info(&commit)
    }

    /// 현재 체크아웃된 브랜치 이름(friendly). detached 면 short sha.
    pub fn current_branch_name(&self) -> Result<String> {
        if let Some(name) = self.repo.head_name()? {
            Ok(name.shorten().to_string())
        } else {
            let commit = self.repo.head_commit()?;
            let sha = commit.id().to_string();
            Ok(sha[..7.min(sha.len())].to_string())
        }
    }

    /// spec(브랜치/태그/sha)을 커밋 ObjectId 로 해석.
    fn resolve(&self, spec: &str) -> Option<ObjectId> {
        let id = self.repo.rev_parse_single(spec).ok()?;
        let commit = id.object().ok()?.try_into_commit().ok()?;
        Some(commit.id)
    }

    /// 모든 태그 수집(가리키는 커밋과 함께).
    pub fn tags(&self) -> Result<Vec<TagInfo>> {
        let mut out = Vec::new();
        let platform = self.repo.references()?;
        for reference in platform.tags()?.flatten() {
            let name = reference.name().shorten().to_string();
            if let Ok(id) = reference.clone().into_fully_peeled_id() {
                let commit = id.object().ok().and_then(|o| o.try_into_commit().ok());
                if let Some(commit) = commit {
                    if let Ok(time) = commit.time() {
                        out.push(TagInfo {
                            name,
                            target_sha: commit.id().to_string(),
                            when: gix_time_to_chrono(time),
                        });
                    }
                }
            }
        }
        Ok(out)
    }

    /// 로컬 + 원격 브랜치 이름 목록(shorthand).
    pub fn branch_names(&self) -> Result<Vec<String>> {
        let mut out = Vec::new();
        let platform = self.repo.references()?;
        for reference in platform.local_branches()?.flatten() {
            out.push(reference.name().shorten().to_string());
        }
        for reference in self.repo.references()?.remote_branches()?.flatten() {
            out.push(reference.name().shorten().to_string());
        }
        Ok(out)
    }

    /// `from`(제외) 부터 `to`(포함) 까지 도달 가능한 커밋들을 최신순으로 반환.
    /// `from` 이 None 이면 `to` 의 모든 조상.
    pub fn commits_between(&self, from: Option<&str>, to: &str) -> Result<Vec<CommitInfo>> {
        let to_oid = self
            .resolve(to)
            .with_context(|| t!("git.commit_not_found", commit = to))?;

        let mut platform = self.repo.rev_walk([to_oid]);
        if let Some(f) = from {
            if let Some(f_oid) = self.resolve(f) {
                platform = platform.with_hidden([f_oid]);
            }
        }

        let mut out = Vec::new();
        for info in platform.all()? {
            let info = info?;
            if let Ok(commit) = self.repo.find_commit(info.id) {
                out.push(Self::commit_info(&commit)?);
            }
        }
        Ok(out)
    }

    /// `from`(제외)부터 `to`(포함)까지 **첫 번째 부모만** 따라가며 커밋을 최신순으로
    /// 반환(Mainline 트렁크 순회용).
    pub fn first_parent_between(&self, from: Option<&str>, to: &str) -> Result<Vec<CommitInfo>> {
        let to_oid = self
            .resolve(to)
            .with_context(|| t!("git.commit_not_found", commit = to))?;
        let mut platform = self.repo.rev_walk([to_oid]).first_parent_only();
        if let Some(f) = from {
            if let Some(f_oid) = self.resolve(f) {
                platform = platform.with_hidden([f_oid]);
            }
        }
        let mut out = Vec::new();
        for info in platform.all()? {
            let info = info?;
            if let Ok(commit) = self.repo.find_commit(info.id) {
                out.push(Self::commit_info(&commit)?);
            }
        }
        Ok(out)
    }

    /// 두 커밋의 merge-base.
    pub fn merge_base(&self, a: &str, b: &str) -> Result<Option<String>> {
        let (oid_a, oid_b) = match (self.resolve(a), self.resolve(b)) {
            (Some(x), Some(y)) => (x, y),
            _ => return Ok(None),
        };
        match self.repo.merge_base(oid_a, oid_b) {
            Ok(base) => Ok(Some(base.to_string())),
            Err(_) => Ok(None),
        }
    }

    /// 특정 커밋이 HEAD 에서 도달 가능한지(조상인지).
    pub fn is_ancestor_of_head(&self, sha: &str) -> Result<bool> {
        let head = self.head_commit()?;
        self.is_ancestor_of(sha, &head.sha)
    }

    /// `ancestor` 가 `descendant` 의 조상(또는 동일)인지.
    pub fn is_ancestor_of(&self, ancestor: &str, descendant: &str) -> Result<bool> {
        let (a, d) = match (self.resolve(ancestor), self.resolve(descendant)) {
            (Some(a), Some(d)) => (a, d),
            _ => return Ok(false),
        };
        if a == d {
            return Ok(true);
        }
        match self.repo.merge_base(a, d) {
            Ok(base) => Ok(base.detach() == a),
            Err(_) => Ok(false),
        }
    }

    /// 커밋이 첫 번째 부모 대비 변경한 파일 경로 목록.
    /// 루트 커밋이거나 diff 를 얻을 수 없으면 빈 벡터를 반환한다.
    pub fn changed_paths_for_commit(&self, sha: &str) -> Vec<String> {
        (|| -> Option<Vec<String>> {
            let oid = self.resolve(sha)?;
            let commit = self.repo.find_commit(oid).ok()?;
            let new_tree = commit.tree().ok()?;
            let parent = commit
                .parent_ids()
                .next()
                .and_then(|pid| self.repo.find_commit(pid).ok())?;
            let old_tree = parent.tree().ok()?;

            let mut paths: Vec<String> = Vec::new();
            let mut platform = old_tree.changes().ok()?;
            // track_path: location() 필드 활성화.
            // track_rewrites(None): rename 추적 비활성화 → blob 접근 불필요.
            platform.options(|o| {
                o.track_path();
                o.track_rewrites(None);
            });
            let _ = platform.for_each_to_obtain_tree(&new_tree, |change| {
                paths.push(change.location().to_string());
                Ok::<_, std::convert::Infallible>(std::ops::ControlFlow::Continue(()))
            });
            Some(paths)
        })()
        .unwrap_or_default()
    }

    /// spec(브랜치/태그/sha)을 CommitInfo 로 해석.
    pub fn commit_info_of(&self, spec: &str) -> Option<CommitInfo> {
        let id = self.resolve(spec)?;
        let commit = self.repo.find_commit(id).ok()?;
        Self::commit_info(&commit).ok()
    }

    /// 로컬 브랜치 이름 목록(shorthand).
    pub fn local_branch_names(&self) -> Result<Vec<String>> {
        let mut out = Vec::new();
        let platform = self.repo.references()?;
        for reference in platform.local_branches()?.flatten() {
            out.push(reference.name().shorten().to_string());
        }
        out.sort();
        Ok(out)
    }

    /// 지정 커밋(기본 HEAD)에 lightweight 태그 생성.
    pub fn create_tag(&self, name: &str, target_spec: Option<&str>) -> Result<()> {
        let target = match target_spec {
            Some(s) => self
                .resolve(s)
                .with_context(|| t!("git.target_commit_not_found").to_string())?,
            None => self.repo.head_commit()?.id,
        };
        self.repo
            .reference(
                format!("refs/tags/{name}"),
                target,
                gix::refs::transaction::PreviousValue::MustNotExist,
                format!("gitversion: create tag {name}"),
            )
            .with_context(|| t!("git.tag_create_failed", name = name))?;
        Ok(())
    }

    /// 지정 커밋(기본 HEAD)에 브랜치 ref 생성(작업 트리는 변경하지 않음).
    pub fn create_branch(&self, name: &str, target_spec: Option<&str>) -> Result<()> {
        let target = match target_spec {
            Some(s) => self
                .resolve(s)
                .with_context(|| t!("git.target_commit_not_found").to_string())?,
            None => self.repo.head_commit()?.id,
        };
        self.repo
            .reference(
                format!("refs/heads/{name}"),
                target,
                gix::refs::transaction::PreviousValue::MustNotExist,
                format!("gitversion: create branch {name}"),
            )
            .with_context(|| t!("git.branch_create_failed", name = name))?;
        Ok(())
    }

    /// 디스크 캐시 디렉터리(`<.git>/gitversion_cache`) 삭제.
    pub fn clear_cache(&self) -> Result<usize> {
        let dir = self.git_dir().join("gitversion_cache");
        if !dir.exists() {
            return Ok(0);
        }
        let count = std::fs::read_dir(&dir).map(|d| d.count()).unwrap_or(0);
        std::fs::remove_dir_all(&dir)
            .with_context(|| t!("git.cache_clear_failed", path = dir.display()))?;
        Ok(count)
    }

    /// 작업 트리의 미커밋 변경 수.
    pub fn uncommitted_changes(&self) -> Result<i64> {
        // 원본 GitVersion 은 HEAD 트리와 (index + working dir) 의 차이를 세며,
        // 여기에는 untracked(추가된) 파일도 포함된다. gix 의 index-worktree 상태는
        // untracked + 수정된 추적 파일을 모두 포함하므로 그 개수를 센다.
        let status = match self.repo.status(gix::progress::Discard) {
            Ok(s) => s,
            Err(_) => return Ok(0),
        };
        let iter = match status.into_index_worktree_iter(Vec::new()) {
            Ok(it) => it,
            Err(_) => return Ok(0),
        };
        Ok(iter.flatten().count() as i64)
    }

    /// 특정 커밋에 직접 붙은 태그들의 이름.
    pub fn tags_on_commit(&self, sha: &str) -> Result<HashSet<String>> {
        Ok(self
            .tags()?
            .into_iter()
            .filter(|t| t.target_sha == sha)
            .map(|t| t.name)
            .collect())
    }
}
