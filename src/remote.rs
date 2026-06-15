//! 동적 원격 저장소 clone.
//!
//! 원본 `GitVersion.Core/Core/GitPreparer.cs` 의 동적 저장소 동작 대응. `/url` 로
//! 원격 저장소를 임시(또는 지정) 위치에 clone 하고 그 위에서 버전을 계산한다.

use anyhow::{bail, Context, Result};
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
        bail!("/url 사용 시 /b(브랜치)가 필요합니다");
    }
    let branch = opts.branch.as_deref().unwrap();

    // clone 대상 경로: <location|%tmp%>/<url-hash>.
    let base = opts.location.clone().unwrap_or_else(std::env::temp_dir);
    let mut hasher = Sha1::new();
    hasher.update(opts.url.as_bytes());
    let hash: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    let dest = base.join(format!("gitversion-dynamic-{hash}"));

    // 항상 깨끗한 상태에서 clone(정확성 우선).
    if dest.exists() {
        std::fs::remove_dir_all(&dest)
            .with_context(|| format!("기존 clone 제거 실패: {}", dest.display()))?;
    }
    std::fs::create_dir_all(&dest)?;

    // 인증 정보가 있으면 https URL 에 주입.
    let url = inject_credentials(&opts.url, opts.username.as_deref(), opts.password.as_deref());

    log::info!("원격 저장소 clone: {} (브랜치 {branch}) -> {}", opts.url, dest.display());

    let should_interrupt = AtomicBool::new(false);
    let mut prepare = gix::prepare_clone(url.as_str(), &dest)
        .with_context(|| format!("clone 준비 실패: {}", opts.url))?
        .with_ref_name(Some(branch))
        .context("브랜치 ref 설정 실패")?;

    let (mut checkout, _) = prepare
        .fetch_then_checkout(gix::progress::Discard, &should_interrupt)
        .context("원격 fetch 실패")?;
    let (_repo, _) = checkout
        .main_worktree(gix::progress::Discard, &should_interrupt)
        .context("작업 트리 체크아웃 실패")?;

    // 특정 커밋(/c) 지정 시 HEAD 를 그 커밋으로 detach.
    if let Some(commit) = &opts.commit {
        detach_head_to_commit(&dest, commit)?;
    }

    Ok(dest)
}

/// https URL 에 user:pass 를 주입(다른 스킴은 그대로).
fn inject_credentials(url: &str, user: Option<&str>, pass: Option<&str>) -> String {
    let Some(user) = user.filter(|u| !u.is_empty()) else { return url.to_string() };
    if let Some(rest) = url.strip_prefix("https://") {
        let cred = match pass.filter(|p| !p.is_empty()) {
            Some(p) => format!("{user}:{p}"),
            None => user.to_string(),
        };
        return format!("https://{cred}@{rest}");
    }
    url.to_string()
}

/// clone 된 저장소의 HEAD 를 지정 커밋으로 detach(.git/HEAD 에 sha 기록).
fn detach_head_to_commit(dest: &std::path::Path, commit: &str) -> Result<()> {
    let repo = gix::open(dest).context("clone 된 저장소 열기 실패")?;
    let id = repo
        .rev_parse_single(commit)
        .with_context(|| format!("커밋을 찾을 수 없습니다: {commit}"))?;
    let full_sha = id.detach().to_string();
    let head_path = repo.git_dir().join("HEAD");
    std::fs::write(&head_path, format!("{full_sha}\n"))
        .with_context(|| format!("HEAD 기록 실패: {}", head_path.display()))?;
    log::info!("HEAD 를 커밋 {full_sha} 로 설정");
    Ok(())
}
