//! Integration tests for `--updatepackagefiles` / `output::files::update_package_files`.
//!
//! Two layers:
//!   1. Library-level: drive `update_package_files` directly against real files on disk
//!      (explicit list, auto-discovery, vendor exclusion, manifests without a version).
//!   2. End-to-end: run the compiled `gitversion-rs` binary with `--updatepackagefiles`
//!      on a temporary git repository and verify the manifests are rewritten.

use std::path::{Path, PathBuf};
use std::process::Command;

use gitversion_rs::output::files::update_package_files;
use gitversion_rs::output::VersionVariables;

/// A temp directory unique to this process + nanos, auto-created.
fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("gv-{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

/// SemVer is what package manifests are stamped with (no build metadata).
fn vars(sem_ver: &str) -> VersionVariables {
    VersionVariables {
        sem_ver: sem_ver.into(),
        ..Default::default()
    }
}

#[test]
fn update_package_files_explicit_list_updates_each_format() {
    let dir = temp_dir("updpkg-explicit");
    write(
        &dir.join("package.json"),
        "{\n  \"name\": \"x\",\n  \"version\": \"0.0.0\",\n  \"private\": true\n}",
    );
    write(
        &dir.join("Cargo.toml"),
        "# keep me\n[package]\nname = \"x\"  # inline\nversion = \"0.0.0\"\n",
    );
    write(
        &dir.join("pyproject.toml"),
        "[project]\nname = \"x\"\nversion = \"0.0.0\"\n",
    );

    let files = vec![
        "package.json".to_string(),
        "Cargo.toml".to_string(),
        "pyproject.toml".to_string(),
    ];
    let updated = update_package_files(&vars("1.2.3-beta.4"), &dir, &files).unwrap();
    assert_eq!(updated.len(), 3, "all three manifests should be updated");

    assert!(read(&dir.join("package.json")).contains("\"version\": \"1.2.3-beta.4\""));
    let cargo = read(&dir.join("Cargo.toml"));
    assert!(cargo.contains("version = \"1.2.3-beta.4\""));
    // Format-preserving: comments survive.
    assert!(cargo.contains("# keep me") && cargo.contains("# inline"));
    assert!(read(&dir.join("pyproject.toml")).contains("version = \"1.2.3-beta.4\""));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn update_package_files_autodiscovery_skips_vendor_and_versionless() {
    let dir = temp_dir("updpkg-auto");
    // Discoverable manifest at the root.
    write(
        &dir.join("Cargo.toml"),
        "[package]\nname = \"root\"\nversion = \"0.0.0\"\n",
    );
    // Vendored manifests that must be ignored.
    write(
        &dir.join("node_modules/dep/package.json"),
        "{\n  \"name\": \"dep\",\n  \"version\": \"0.0.0\"\n}",
    );
    write(
        &dir.join("target/pkg/Cargo.toml"),
        "[package]\nname = \"built\"\nversion = \"0.0.0\"\n",
    );
    // A manifest with no `version` field must be left untouched (and not counted).
    write(
        &dir.join("workspace/Cargo.toml"),
        "[workspace]\nmembers = [\"a\"]\n",
    );

    // Empty list → recursive auto-discovery.
    let updated = update_package_files(&vars("9.9.9"), &dir, &[]).unwrap();

    // Only the root Cargo.toml qualifies.
    assert_eq!(
        updated.len(),
        1,
        "only the root manifest should update, got {updated:?}"
    );
    assert!(read(&dir.join("Cargo.toml")).contains("version = \"9.9.9\""));
    // Vendored + versionless manifests stay at their original content.
    assert!(read(&dir.join("node_modules/dep/package.json")).contains("\"version\": \"0.0.0\""));
    assert!(read(&dir.join("target/pkg/Cargo.toml")).contains("version = \"0.0.0\""));
    assert!(!read(&dir.join("workspace/Cargo.toml")).contains("version ="));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn update_package_files_cargo_workspace_layout() {
    // Mirrors a Cargo workspace (e.g. git-warden): the version lives in the root's
    // [workspace.package], and members inherit via `version.workspace = true`.
    let dir = temp_dir("updpkg-workspace");
    write(
        &dir.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/*\"]\n\n[workspace.package]\nversion = \"0.0.1\"\n",
    );
    write(
        &dir.join("crates/git-warden/Cargo.toml"),
        "[package]\nname = \"git-warden\"\nversion.workspace = true\n",
    );

    let updated = update_package_files(&vars("3.1.4"), &dir, &[]).unwrap();

    // Only the workspace root is rewritten; the inheriting member is left alone.
    assert_eq!(
        updated.len(),
        1,
        "only the workspace root should update: {updated:?}"
    );
    assert!(read(&dir.join("Cargo.toml")).contains("version = \"3.1.4\""));
    // Member keeps inheritance — no hard-coded version was injected.
    let member = read(&dir.join("crates/git-warden/Cargo.toml"));
    assert!(member.contains("version.workspace = true"));
    assert!(!member.contains("version = \"3.1.4\""));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn update_package_files_npm_workspace_layout() {
    // npm-style workspace: the private root has no version (skipped), members do.
    let dir = temp_dir("updpkg-npm");
    write(
        &dir.join("package.json"),
        "{\n  \"name\": \"root\",\n  \"private\": true,\n  \"workspaces\": [\"packages/*\"]\n}",
    );
    write(
        &dir.join("packages/app/package.json"),
        "{\n  \"name\": \"app\",\n  \"version\": \"0.0.0\"\n}",
    );

    let updated = update_package_files(&vars("4.2.0"), &dir, &[]).unwrap();

    assert_eq!(
        updated.len(),
        1,
        "only the versioned member should update: {updated:?}"
    );
    // Root (no version) untouched; member bumped.
    assert!(!read(&dir.join("package.json")).contains("\"version\""));
    assert!(read(&dir.join("packages/app/package.json")).contains("\"version\": \"4.2.0\""));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn update_package_files_pyproject_variants() {
    // PEP 621 static, Poetry, and PEP 621 dynamic (skipped) side by side.
    let dir = temp_dir("updpkg-py");
    write(
        &dir.join("pep621/pyproject.toml"),
        "[project]\nname = \"a\"\nversion = \"0.0.0\"\n",
    );
    write(
        &dir.join("poetry/pyproject.toml"),
        "[tool.poetry]\nname = \"b\"\nversion = \"0.0.0\"\n",
    );
    write(
        &dir.join("dynamic/pyproject.toml"),
        "[project]\nname = \"c\"\ndynamic = [\"version\"]\n",
    );

    let updated = update_package_files(&vars("7.0.0"), &dir, &[]).unwrap();

    assert_eq!(
        updated.len(),
        2,
        "static + poetry update, dynamic skipped: {updated:?}"
    );
    assert!(read(&dir.join("pep621/pyproject.toml")).contains("version = \"7.0.0\""));
    assert!(read(&dir.join("poetry/pyproject.toml")).contains("version = \"7.0.0\""));
    assert!(!read(&dir.join("dynamic/pyproject.toml")).contains("7.0.0"));

    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// End-to-end: the real CLI binary updating manifests in a git repository.
// ---------------------------------------------------------------------------

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_AUTHOR_DATE", "1609459200 +0000")
        .env("GIT_COMMITTER_DATE", "1609459200 +0000")
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .status()
        .expect("failed to run git");
    assert!(status.success(), "git {args:?} failed");
}

#[test]
fn cli_updatepackagefiles_end_to_end() {
    let dir = temp_dir("updpkg-cli");
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "commit.gpgsign", "false"]);
    git(&dir, &["config", "user.email", "test@example.com"]);
    git(&dir, &["config", "user.name", "test"]);

    // Placeholder manifests (version 0.0.0) committed to the repo.
    write(
        &dir.join("Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.0.0\"\n",
    );
    write(
        &dir.join("package.json"),
        "{\n  \"name\": \"app\",\n  \"version\": \"0.0.0\"\n}",
    );
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "--no-verify", "-m", "chore: init"]);
    // Tag HEAD so the computed version is deterministic regardless of defaults.
    git(&dir, &["tag", "v1.4.0"]);

    // Run: update package files AND print the SemVer the CLI used (file updates run
    // before -showvariable in the pipeline, so both happen in one invocation).
    let bin = env!("CARGO_BIN_EXE_gitversion-rs");
    let out = Command::new(bin)
        .current_dir(&dir)
        .args([
            ".",
            "--nocache",
            "--updatepackagefiles",
            "--showvariable",
            "SemVer",
        ])
        .output()
        .expect("failed to run gitversion-rs");
    assert!(
        out.status.success(),
        "CLI failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let version = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert_eq!(version, "1.4.0", "tagged HEAD should yield the tag version");

    // Both manifests must now carry the computed version.
    assert!(
        read(&dir.join("Cargo.toml")).contains("version = \"1.4.0\""),
        "Cargo.toml not updated: {}",
        read(&dir.join("Cargo.toml"))
    );
    assert!(
        read(&dir.join("package.json")).contains("\"version\": \"1.4.0\""),
        "package.json not updated: {}",
        read(&dir.join("package.json"))
    );

    std::fs::remove_dir_all(&dir).ok();
}
