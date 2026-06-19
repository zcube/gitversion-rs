# AGENTS.md

Guide for agents/contributors working in this repository. For a user-facing overview see the
[README](README.md) (translations: [ko](README.ko.md) · [ja](README.ja.md) · [zh](README.zh.md)).

## Project overview

A Rust port of GitVersion (.NET) as a single native binary. It computes a SemVer from Git
history. Pure Rust (gix), verified against the real GitVersion 6.x binary via differential tests.

## Development workflow

```bash
cargo build                  # build
cargo test                   # unit + fixture integration tests
cargo fmt --all              # format
cargo clippy --all-targets -- -D warnings   # lint (keep zero warnings)
```

- **lefthook** (install: `lefthook install`)
  - pre-commit: `cargo fmt` (auto-format + re-stage), `clippy -D warnings` on staged `*.rs`, `git-warden diff`
  - commit-msg: `git-warden msg` (language + policy check)
  - prepare-commit-msg: `git-warden prepare-msg`
  - pre-push: `cargo test`
- **CI** (`.github/workflows/ci.yml`): fmt --check, clippy -D warnings, build/test on 3 OSes,
  MSRV build (currently 1.88, the transitive-dependency floor). Runs on pushes to main and PRs.

## Commit conventions

- **Conventional Commits** type prefix required: `feat|fix|ci|chore|test|docs|refactor|perf|style|build|revert`.
- Commit messages must be written in **English** (enforced by `git-warden` via the commit-msg hook).
- **No AI co-author trailers** (e.g. Co-Authored-By), and no special characters such as arrows or
  emoji (the commit-msg hook rejects them).
- For throwaway test repos, `git commit --no-verify` is fine.

## i18n

- Crate: [`rust-i18n`](https://github.com/longbridge/rust-i18n). Default language is **English**,
  source keys are English. Supported: en/ko/ja/zh.
- Translations live in `locales/app.yml` (`_version: 2`) as `key: { en, ko, ja, zh }` or block form.
  Placeholders use `%{name}`.
- In code: `rust_i18n::t!("key", name = value)`. **`t!` only works inside the lib crate
  (`src/lib.rs`) that invokes the `i18n!` macro**, so the entry logic lives in `src/app.rs`
  (`main.rs` is a thin shim).
- Pass runtime variable keys with `t!(*k)` (double-ref deref). CLI help is injected before parsing
  by `cli::localized_command()` via the `cli.about` / `cli.help.<arg_id>` keys.
- **When you add a user-facing string, always add all four language values to `locales/app.yml`.**
  On a missing key, rust-i18n prints the key string verbatim.

## Versioning and releases

This project computes its own version with itself (dogfooding).

- **The single source of truth for the version is the git tag (`v*`)** plus `GitVersion.yml`.
- `Cargo.toml`'s `version` is a **placeholder**. Do not bump it by hand. The release build
  **overwrites it automatically** with the value gitversion-rs computes, so the distributed
  binary's `--version` reports the real release version.
- Untagged dev builds derive a pre-release like `0.1.0-<distance>` from `GitVersion.yml`'s
  `next-version` (currently 0.1.0).

### Release procedure

1. Make sure `main` is green (CI passing).
2. Decide the next release version. Optionally check the candidate with gitversion-rs:
   ```bash
   cargo run -q -- -v SemVer        # current (pre-tag) version
   ```
3. Create an **annotated tag** on the release commit and push it (the tag is the version source):
   ```bash
   git tag -a v0.1.0 -m "release: v0.1.0"
   git push origin v0.1.0
   ```
4. The tag push triggers `.github/workflows/release.yml`:
   - **version job**: builds and runs gitversion-rs at the tagged commit to compute `SemVer`
     (with the tag present, `v0.1.0 -> 0.1.0`).
   - **build job** (6 targets: Linux x86_64/aarch64/musl, macOS x86_64/aarch64, Windows x86_64):
     injects the computed version into `Cargo.toml`, builds, and uploads the archive (the four
     READMEs + LICENSE included) to the GitHub Release.
5. Result: per-platform binaries are attached to the GitHub Release, and each binary's
   `gitversion-rs --version` reports the tag version.
   - **checksums job**: generates `checksums.txt` (SHA256 of all binaries), keyless-signs it with
     cosign (Sigstore, via GitHub Actions OIDC — no key/secret needed) producing
     `checksums.txt.sigstore.json`, uploads both, and writes verification instructions into the
     release notes. Verify with `sha256sum -c` and `cosign verify-blob`.
6. **Homebrew tap auto-update**: after the build, the `homebrew` job computes the assets' SHA256
   and overwrites `Formula/gitversion-rs.rb` in `zcube/homebrew-tap` with the new version, then
   commits and pushes.
   - Required secret: **`HOMEBREW_TAP_TOKEN`** — a PAT (or fine-grained token) with contents:write
     on `zcube/homebrew-tap`. Add it to the gitversion-rs repository Secrets.
   - If the secret is absent, this job is silently skipped (the release itself is unaffected).
   - Pre-releases (version containing `-`, e.g. `0.1.0-rc.1`) do not update the tap.
   - Install: `brew install zcube/tap/gitversion-rs` (command: `gitversion-rs`).
   - The package, binary, command, and formula are all unified as `gitversion-rs` (to avoid
     clashing with the official .NET GitVersion `gitversion`). Release archives are named
     `gitversion-rs-<tag>-<target>` and the inner executable is `gitversion-rs` too.

> For a manual rebuild, run the `Release` workflow via `workflow_dispatch` and provide the tag.

### Bumping the version

- To change the next cycle's baseline, update `next-version` in `GitVersion.yml`, or simply create
  a higher `v*` tag (the tag always wins).

## Recommended pattern for consuming Rust projects

This section documents the recommended way to integrate gitversion-rs into a Rust project.

### Tool roles

| Tool | Responsibility |
|---|---|
| **cargo-release** | Version number management — bumps `Cargo.toml`, commits, tags |
| **gitversion-rs `--exec`** | Build-time metadata injection — sets `CARGO_PKG_VERSION_PRE`, `GitVersion_*` env vars |

`Cargo.toml` is the source of truth for the version number. `gitversion-rs` provides the
`PreReleaseTag` and `InformationalVersion` (SHA + branch) at build time without touching the commit
history.

### How `--exec` works

`gitversion-rs --exec "cargo build"` (or the `exec.prepare` hook in `GitVersion.yml`) runs the
given command with the following environment variables pre-set in the child process:

```
CARGO_PKG_VERSION       = SemVer          (e.g. "0.2.0" or "0.2.0-alpha.1")
CARGO_PKG_VERSION_MAJOR = Major
CARGO_PKG_VERSION_MINOR = Minor
CARGO_PKG_VERSION_PATCH = Patch
CARGO_PKG_VERSION_PRE   = PreReleaseTag   (e.g. "" on a release tag, "5" on an untagged commit)
GitVersion_SemVer       = SemVer
GitVersion_InformationalVersion = "0.2.0+Branch.main.Sha.abc1234"
... (all GitVersion_* variables)
```

`CARGO_PKG_VERSION_PRE` is the key variable: it carries the `PreReleaseTag` computed from git
history, so a release build gets `""` and a dev build gets the commit distance (`"5"`) or branch
tag (`"alpha.1"`).

### build.rs pattern

```rust
fn main() {
    // gitversion-rs --exec sets GitVersion_InformationalVersion; fall back to CARGO_PKG_VERSION.
    let info = std::env::var("GitVersion_InformationalVersion")
        .unwrap_or_else(|_| std::env::var("CARGO_PKG_VERSION").unwrap_or_default());

    println!("cargo:rustc-env=APP_INFO_VERSION={info}");
    println!("cargo:rerun-if-env-changed=GitVersion_InformationalVersion");
}
```

`CARGO_PKG_VERSION_PRE` is available in source files directly without a `build.rs`:

```rust
const PRE: &str = env!("CARGO_PKG_VERSION_PRE");  // "" on release, "5" on dev, "alpha.1" on pre-release
```

### Release workflow

```bash
# 1. Bump version, commit, and tag — Cargo.toml becomes the record
cargo release minor    # 0.1.x → 0.2.0: updates Cargo.toml, commits "chore: release v0.2.0", tags v0.2.0

# 2. Build with gitversion-rs injecting CARGO_PKG_VERSION_PRE and InformationalVersion
gitversion-rs --exec "cargo build --release"
```

In CI:

```yaml
- run: gitversion-rs --exec "cargo build --release"
```

No `--allow-dirty` or separate version-injection step is needed: `cargo-release` has already set
the correct version in `Cargo.toml` before the build starts.

### Fallback hierarchy (no git / no gitversion-rs)

`crates.io` installs have no git repository. In that case:
- `GitVersion_*` env vars are absent → `build.rs` falls back to `CARGO_PKG_VERSION`
- `CARGO_PKG_VERSION_PRE` is set by Cargo from `Cargo.toml` (e.g. `""` for a release)
- The binary version is always correct because `cargo-release` stamped the right version into
  `Cargo.toml` before publishing

## Dependency updates (Renovate)

- `.github/renovate.json` drives Renovate. Schedule: before 9am on Monday; concurrent PR limit 5.
- Groups: gix (gitoxide) crates, dev-dependencies (automerge), GitHub Actions (automerge);
  `lockFileMaintenance` refreshes `Cargo.lock`.
- Major bumps may break the build; fix them locally, verify green, and push. PRs merge only after
  CI passes.

## Code layout

| Module | Role |
|---|---|
| `src/git` | gix-based repository access |
| `src/config` | config model / defaults / loader / effective |
| `src/version` | SemanticVersion and the calculation engine |
| `src/output` | output variables / formatters / file output |
| `src/cli` | clap arguments + `localized_command()` |
| `src/app.rs` | entry logic (kept inside the lib so `t!` works) |
| `src/tui` | ratatui TUI |
| `src/i18n.rs` + `locales/` | rust-i18n locale handling |

> `refs/gitversion` is the .NET source this port was based on; it is excluded from tracking via
> `.gitignore`.
