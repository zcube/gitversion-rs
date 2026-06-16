//! git 조작 API(태그/브랜치 생성, 캐시 삭제) 통합 테스트.
//!
//! `git` CLI 로 임시 저장소를 만든 뒤 GitRepo 로 조작을 검증한다.

use std::path::Path;
use std::process::Command;

use gitversion::git::GitRepo;

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_AUTHOR_DATE", "1609459200 +0000")
        .env("GIT_COMMITTER_DATE", "1609459200 +0000")
        .status()
        .expect("git 실행 실패");
    assert!(status.success(), "git {args:?} 실패");
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

    // 태그 생성 → HEAD 에 존재.
    repo.create_tag("v9.9.9", None).unwrap();
    let tags = repo.tags().unwrap();
    assert!(
        tags.iter().any(|t| t.name == "v9.9.9"),
        "태그가 생성되지 않음: {tags:?}"
    );

    // 브랜치 생성 → 로컬 브랜치 목록에 포함.
    repo.create_branch("feature/from-tui", None).unwrap();
    let branches = repo.local_branch_names().unwrap();
    assert!(
        branches.iter().any(|b| b == "feature/from-tui"),
        "브랜치 미생성: {branches:?}"
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
    // 없을 때 호출은 0.
    assert_eq!(repo.clear_cache().unwrap(), 0);

    std::fs::remove_dir_all(&dir).ok();
}
