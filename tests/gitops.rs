//! Integration tests for the git manipulation API (tag/branch creation, cache clearing).
//!
//! Creates a temporary repository via the `git` CLI and verifies operations through `GitRepo`.

use std::path::Path;
use std::process::Command;

use gitversion_rs::git::GitRepo;

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_AUTHOR_DATE", "1609459200 +0000")
        .env("GIT_COMMITTER_DATE", "1609459200 +0000")
        // Provide an explicit identity because CI runners may not have a global git config.
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .status()
        .expect("failed to run git");
    assert!(status.success(), "git {args:?} failed");
}

fn temp_repo() -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("gv-gitops-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "commit.gpgsign", "false"]);
    // Set local identity so gix can read the committer when writing the reflog for branch creation.
    // (CI runners may not have a global git identity configured.)
    git(&dir, &["config", "user.email", "test@example.com"]);
    git(&dir, &["config", "user.name", "test"]);
    git(
        &dir,
        &["commit", "-q", "--no-verify", "--allow-empty", "-m", "a"],
    );
    dir
}

#[test]
fn create_tag_and_branch_via_gix() {
    let dir = temp_repo();
    let repo = GitRepo::discover(&dir).unwrap();

    // Create tag → it must appear on HEAD.
    repo.create_tag("v9.9.9", None).unwrap();
    let tags = repo.tags().unwrap();
    assert!(
        tags.iter().any(|t| t.name == "v9.9.9"),
        "tag was not created: {tags:?}"
    );

    // Create branch → it must appear in the local branch list.
    repo.create_branch("feature/from-tui", None).unwrap();
    let branches = repo.local_branch_names().unwrap();
    assert!(
        branches.iter().any(|b| b == "feature/from-tui"),
        "branch was not created: {branches:?}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn clear_cache_removes_dir() {
    let dir = temp_repo();
    let repo = GitRepo::discover(&dir).unwrap();
    let cache_dir = dir.join(".git/gitversion_cache");
    std::fs::create_dir_all(&cache_dir).unwrap();
    std::fs::write(cache_dir.join("x.json"), "{}").unwrap();

    let removed = repo.clear_cache().unwrap();
    assert_eq!(removed, 1);
    assert!(!cache_dir.exists());
    // Calling again when nothing is cached returns 0.
    assert_eq!(repo.clear_cache().unwrap(), 0);

    std::fs::remove_dir_all(&dir).ok();
}
