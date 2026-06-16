//! 동적 원격 저장소 clone.
//!
//! 원본 `GitVersion.Core/Core/GitPreparer.cs` 의 동적 저장소 동작 대응. `/url` 로
//! 원격 저장소를 임시(또는 지정) 위치에 clone 하고 그 위에서 버전을 계산한다.
//!
//! 전송: gix 의 blocking 클라이언트로 https/file 과 SSH(`ssh://` 및 scp-like
//! `git@host:path`)를 지원한다. SSH 는 시스템 `ssh` 명령(키·에이전트)을 사용한다.

use anyhow::{bail, Context, Result};
use rust_i18n::t;
use sha1::{Digest, Sha1};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

/// 동적 clone 옵션.
pub struct DynamicRepoOptions {
    pub url: String,
    pub branch: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub commit: Option<String>,
    pub location: Option<PathBuf>,
}

/// 원격 저장소를 clone(또는 재사용)하고 대상 브랜치/커밋을 체크아웃한 뒤, 그
/// 작업 트리 경로를 반환한다.
pub fn prepare(opts: &DynamicRepoOptions) -> Result<PathBuf> {
    if opts.branch.is_none() {
        bail!("{}", t!("remote.branch_required"));
    }
    let branch = opts.branch.as_deref().unwrap();

    // clone 대상 경로: <location|%tmp%>/<url-hash>.
    let base = opts.location.clone().unwrap_or_else(std::env::temp_dir);
    let mut hasher = Sha1::new();
    hasher.update(opts.url.as_bytes());
    let hash: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let dest = base.join(format!("gitversion-dynamic-{hash}"));

    // 항상 깨끗한 상태에서 clone(정확성 우선).
    if dest.exists() {
        std::fs::remove_dir_all(&dest)
            .with_context(|| t!("remote.remove_failed", path = dest.display()))?;
    }
    std::fs::create_dir_all(&dest)?;

    // 인증 정보가 있으면 https URL 에 주입.
    let url = inject_credentials(
        &opts.url,
        opts.username.as_deref(),
        opts.password.as_deref(),
    );

    log::info!(
        "{}",
        t!(
            "remote.cloning",
            url = opts.url,
            branch = branch,
            dest = dest.display()
        )
    );

    let should_interrupt = AtomicBool::new(false);
    let mut prepare = gix::prepare_clone(url.as_str(), &dest)
        .with_context(|| t!("remote.clone_prepare_failed", url = opts.url))?
        .with_ref_name(Some(branch))
        .with_context(|| t!("remote.set_ref_failed").to_string())?;

    let (mut checkout, _) = prepare
        .fetch_then_checkout(gix::progress::Discard, &should_interrupt)
        .with_context(|| t!("remote.fetch_failed").to_string())?;
    let (_repo, _) = checkout
        .main_worktree(gix::progress::Discard, &should_interrupt)
        .with_context(|| t!("remote.checkout_failed").to_string())?;

    // 특정 커밋(/c) 지정 시 HEAD 를 그 커밋으로 detach.
    if let Some(commit) = &opts.commit {
        detach_head_to_commit(&dest, commit)?;
    }

    Ok(dest)
}

/// 인증 정보를 URL 에 반영.
/// - https: `user[:pass]@` 주입
/// - ssh(`ssh://host`): 사용자가 URL 에 없으면 `user@` 주입(SSH 는 키/에이전트 인증)
/// - scp-like(`git@host:path`) 등 이미 사용자가 포함된 형태는 그대로.
fn inject_credentials(url: &str, user: Option<&str>, pass: Option<&str>) -> String {
    let Some(user) = user.filter(|u| !u.is_empty()) else {
        return url.to_string();
    };
    if let Some(rest) = url.strip_prefix("https://") {
        let cred = match pass.filter(|p| !p.is_empty()) {
            Some(p) => format!("{user}:{p}"),
            None => user.to_string(),
        };
        return format!("https://{cred}@{rest}");
    }
    if let Some(rest) = url.strip_prefix("ssh://") {
        // 이미 user@ 가 있으면 그대로.
        let host_part = rest.split('/').next().unwrap_or(rest);
        if !host_part.contains('@') {
            return format!("ssh://{user}@{rest}");
        }
    }
    url.to_string()
}

/// clone 된 저장소의 HEAD 를 지정 커밋으로 detach(.git/HEAD 에 sha 기록).
fn detach_head_to_commit(dest: &std::path::Path, commit: &str) -> Result<()> {
    let repo = gix::open(dest).with_context(|| t!("remote.open_failed").to_string())?;
    let id = repo
        .rev_parse_single(commit)
        .with_context(|| t!("git.commit_not_found", commit = commit))?;
    let full_sha = id.detach().to_string();
    let head_path = repo.git_dir().join("HEAD");
    std::fs::write(&head_path, format!("{full_sha}\n"))
        .with_context(|| t!("remote.head_write_failed", path = head_path.display()))?;
    log::info!("{}", t!("remote.head_set", sha = full_sha));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::inject_credentials;

    #[test]
    fn https_injects_user_and_pass() {
        assert_eq!(
            inject_credentials("https://host/r.git", Some("u"), Some("p")),
            "https://u:p@host/r.git"
        );
        assert_eq!(
            inject_credentials("https://host/r.git", Some("u"), None),
            "https://u@host/r.git"
        );
    }

    #[test]
    fn ssh_injects_user_when_absent() {
        assert_eq!(
            inject_credentials("ssh://host/r.git", Some("git"), None),
            "ssh://git@host/r.git"
        );
        // 이미 user@ 가 있으면 그대로.
        assert_eq!(
            inject_credentials("ssh://git@host/r.git", Some("other"), None),
            "ssh://git@host/r.git"
        );
    }

    #[test]
    fn scp_like_and_no_user_unchanged() {
        assert_eq!(
            inject_credentials("git@host:r.git", Some("u"), None),
            "git@host:r.git"
        );
        assert_eq!(
            inject_credentials("https://host/r.git", None, None),
            "https://host/r.git"
        );
    }
}
