//! Pure-Rust repository access layer built on gix (gitoxide).
//!
//! Corresponds to the original `GitVersion.LibGit2Sharp`, providing the minimum graph
//! operations needed for version calculation: tag collection, commit walking, merge-base, and uncommitted changes.

use anyhow::{Context, Result};
use chrono::{DateTime, FixedOffset, TimeZone};
use gix::ObjectId;
use rust_i18n::t;
use std::collections::HashSet;
use std::path::Path;

/// Summary of a single commit.
#[derive(Debug, Clone)]
pub struct CommitInfo {
    pub sha: String,
    pub short_sha: String,
    pub message: String,
    pub when: DateTime<FixedOffset>,
    pub parent_count: usize,
    /// List of parent commit SHAs.
    pub parents: Vec<String>,
}

/// A candidate version tag.
#[derive(Debug, Clone)]
pub struct TagInfo {
    pub name: String,
    /// The commit SHA the tag points to (peeled for annotated tags).
    pub target_sha: String,
    pub when: DateTime<FixedOffset>,
}

/// Repository wrapper.
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
    /// Discover and open the repository by searching `path` and its parents for `.git`.
    pub fn discover(path: &Path) -> Result<Self> {
        let repo =
            gix::discover(path).with_context(|| t!("git.repo_not_found", path = path.display()))?;
        Ok(Self { repo })
    }

    /// Root of the repository working tree.
    pub fn workdir(&self) -> Option<&Path> {
        self.repo.workdir()
    }

    /// Path to the `.git` directory (used to compute the cache location).
    pub fn git_dir(&self) -> &Path {
        self.repo.git_dir()
    }

    /// Canonical ref name of HEAD (or the short SHA when detached).
    pub fn head_ref_name(&self) -> String {
        match self.repo.head_name() {
            Ok(Some(name)) => name.as_bstr().to_string(),
            _ => self
                .head_commit()
                .map(|c| c.short_sha)
                .unwrap_or_else(|_| "HEAD".into()),
        }
    }

    /// Sorted list of `"<name> <target_sha>"` for every ref. Used as the refs snapshot in the cache key.
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

    /// The commit that HEAD points to.
    pub fn head_commit(&self) -> Result<CommitInfo> {
        let commit = self
            .repo
            .head_commit()
            .with_context(|| t!("git.head_read").to_string())?;
        Self::commit_info(&commit)
    }

    /// Friendly name of the currently checked-out branch.
    ///
    /// When HEAD is detached, mirrors the original GitVersion (`GitVersionContextFactory`)
    /// `GetBranchesContainingCommit(...).OnlyOrDefault()` logic: if a branch has HEAD as its
    /// tip (direct match), that branch is used; otherwise the first branch that contains HEAD
    /// as a reachable ancestor is used. Exactly one match returns its name; zero or multiple
    /// matches return `(no branch)`. (When CI checks out a tag as detached HEAD, HEAD may not
    /// be the tip of main, so tip-only matching is insufficient.)
    pub fn current_branch_name(&self) -> Result<String> {
        if let Some(name) = self.repo.head_name()? {
            Ok(name.shorten().to_string())
        } else {
            let head_sha = self.repo.head_commit()?.id().to_string();
            let containing = self.branches_containing(&head_sha);
            if containing.len() == 1 {
                Ok(containing.into_iter().next().unwrap())
            } else {
                Ok("(no branch)".to_string())
            }
        }
    }

    /// Local branch names (shorthand) whose tip is the given SHA.
    fn local_branches_at(&self, sha: &str) -> Vec<String> {
        let mut out = Vec::new();
        if let Ok(platform) = self.repo.references() {
            if let Ok(branches) = platform.local_branches() {
                for reference in branches.flatten() {
                    if let Ok(id) = reference.clone().into_fully_peeled_id() {
                        if id.to_string() == sha {
                            out.push(reference.name().shorten().to_string());
                        }
                    }
                }
            }
        }
        out
    }

    /// Mirrors the original `GetBranchesContainingCommit`: returns local branches whose tip is
    /// HEAD (direct match); if none, returns local branches that contain HEAD as a reachable
    /// ancestor. Only local branches are considered (matching the original's tracked-branch priority).
    fn branches_containing(&self, head_sha: &str) -> Vec<String> {
        let direct = self.local_branches_at(head_sha);
        if !direct.is_empty() {
            return direct;
        }
        let mut out = Vec::new();
        if let Ok(platform) = self.repo.references() {
            if let Ok(branches) = platform.local_branches() {
                for reference in branches.flatten() {
                    if let Ok(id) = reference.clone().into_fully_peeled_id() {
                        let tip = id.to_string();
                        // HEAD is an ancestor of this branch tip → the branch contains HEAD.
                        if self.is_ancestor_of(head_sha, &tip).unwrap_or(false) {
                            out.push(reference.name().shorten().to_string());
                        }
                    }
                }
            }
        }
        out
    }

    /// Resolve a spec (branch/tag/SHA) to a commit ObjectId.
    fn resolve(&self, spec: &str) -> Option<ObjectId> {
        let id = self.repo.rev_parse_single(spec).ok()?;
        let commit = id.object().ok()?.try_into_commit().ok()?;
        Some(commit.id)
    }

    /// Collect all tags together with the commits they point to.
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

    /// Shorthand names of all local and remote branches.
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

    /// Returns reachable commits from `from` (exclusive) to `to` (inclusive), newest first.
    /// When `from` is `None`, returns all ancestors of `to`.
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

    /// Returns commits from `from` (exclusive) to `to` (inclusive) following **first parents only**,
    /// newest first. Used for Mainline trunk traversal.
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

    /// Merge-base of two commits.
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

    /// Returns true if the given commit is reachable from HEAD (i.e. is an ancestor).
    pub fn is_ancestor_of_head(&self, sha: &str) -> Result<bool> {
        let head = self.head_commit()?;
        self.is_ancestor_of(sha, &head.sha)
    }

    /// Returns true if `ancestor` is an ancestor of (or identical to) `descendant`.
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

    /// File paths changed by a commit relative to its first parent.
    /// Returns an empty vec for root commits or when the diff cannot be obtained.
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
            // track_path: enables the location() field.
            // track_rewrites(None): disables rename tracking → no blob access needed.
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

    /// Resolve a spec (branch/tag/SHA) to a `CommitInfo`.
    pub fn commit_info_of(&self, spec: &str) -> Option<CommitInfo> {
        let id = self.resolve(spec)?;
        let commit = self.repo.find_commit(id).ok()?;
        Self::commit_info(&commit).ok()
    }

    /// Sorted list of shorthand local branch names.
    pub fn local_branch_names(&self) -> Result<Vec<String>> {
        let mut out = Vec::new();
        let platform = self.repo.references()?;
        for reference in platform.local_branches()?.flatten() {
            out.push(reference.name().shorten().to_string());
        }
        out.sort();
        Ok(out)
    }

    /// Create a lightweight tag on the specified commit (defaults to HEAD).
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

    /// Create a branch ref on the specified commit (defaults to HEAD). Does not touch the working tree.
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

    /// Delete the on-disk cache directory (`<.git>/gitversion_cache`).
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

    /// Number of uncommitted changes in the working tree.
    pub fn uncommitted_changes(&self) -> Result<i64> {
        // The original GitVersion counts the diff between the HEAD tree and (index + working dir),
        // including untracked (added) files. gix's index-worktree status covers both untracked
        // and modified tracked files, so we count that.
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

    /// Names of tags directly attached to the given commit.
    pub fn tags_on_commit(&self, sha: &str) -> Result<HashSet<String>> {
        Ok(self
            .tags()?
            .into_iter()
            .filter(|t| t.target_sha == sha)
            .map(|t| t.name)
            .collect())
    }
}
