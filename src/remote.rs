//! Dynamic remote repository cloning.
//!
//! Corresponds to the dynamic-repository behaviour in `GitVersion.Core/Core/GitPreparer.cs`.
//! Clones a remote repository to a temporary (or specified) location via `--url`
//! and computes the version from the clone.
//!
//! Transport: uses gix's blocking client. Supports https/file and SSH
//! (`ssh://` and scp-style `git@host:path`). SSH authentication relies on the
//! system `ssh` command (key files and agent).

use anyhow::{bail, Context, Result};
use rust_i18n::t;
use sha1::{Digest, Sha1};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

/// Options for a dynamic clone.
pub struct DynamicRepoOptions {
    pub url: String,
    pub branch: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub commit: Option<String>,
    pub location: Option<PathBuf>,
}

/// Clone a remote repository (always fresh) and check out the target branch/commit,
/// then return the path to the working tree.
pub fn prepare(opts: &DynamicRepoOptions) -> Result<PathBuf> {
    if opts.branch.is_none() {
        bail!("{}", t!("remote.branch_required"));
    }
    let branch = opts.branch.as_deref().unwrap();

    // Clone destination: <location|%tmp%>/<url-hash>.
    let base = opts.location.clone().unwrap_or_else(std::env::temp_dir);
    let mut hasher = Sha1::new();
    hasher.update(opts.url.as_bytes());
    let hash: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let dest = base.join(format!("gitversion-dynamic-{hash}"));

    // Always clone fresh (correctness over performance).
    if dest.exists() {
        std::fs::remove_dir_all(&dest)
            .with_context(|| t!("remote.remove_failed", path = dest.display()))?;
    }
    std::fs::create_dir_all(&dest)?;

    // Inject credentials into the https URL if provided.
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

    // If a specific commit (/c) is given, detach HEAD to that commit.
    if let Some(commit) = &opts.commit {
        detach_head_to_commit(&dest, commit)?;
    }

    Ok(dest)
}

/// Inject credentials into the URL.
/// - https: prepend `user[:pass]@`
/// - ssh (`ssh://host`): prepend `user@` if no user is already present (SSH uses key/agent auth)
/// - scp-style (`git@host:path`) or URLs that already contain a user are returned unchanged.
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
        // Leave unchanged if user@ is already present.
        let host_part = rest.split('/').next().unwrap_or(rest);
        if !host_part.contains('@') {
            return format!("ssh://{user}@{rest}");
        }
    }
    url.to_string()
}

/// Detach HEAD of the cloned repository to the specified commit (writes the SHA to `.git/HEAD`).
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
        // Already has user@ — leave unchanged.
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
