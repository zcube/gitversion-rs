# gitversion-rs (Rust port)

**English** · [한국어](README.ko.md) · [日本語](README.ja.md) · [中文](README.zh.md)

A Rust port of [GitVersion](https://gitversion.net) (.NET). It computes a Semantic
Version (SemVer) from your Git history.

> **Project goal: run GitVersion in environments without .NET, with minimal effort.**
> A single self-contained native binary — no .NET runtime, no global tool install.
> Pure-Rust Git access (no libgit2/C dependency), differentially verified against the
> real GitVersion binary.

The CLI, the interactive TUI, and all internal messages are **fully internationalized**
(English / Korean / Japanese / Chinese) via [`rust-i18n`](https://github.com/longbridge/rust-i18n).
English is the default; override with `--lang ko|ja|zh` or the `LANG`/`LC_ALL` environment
variable.

## Features

- **Pure-Rust Git access**: [`gix`](https://github.com/GitoxideLabs/gitoxide) (gitoxide) — no libgit2/C dependency
- **CLI**: [`clap`](https://docs.rs/clap)
- **Logging**: [`env_logger`](https://docs.rs/env_logger) (`RUST_LOG`, or `--verbosity`/`--diag`)
- **i18n**: [`rust-i18n`](https://github.com/longbridge/rust-i18n), default English, `--lang`/`LANG`, locales in `locales/app.yml`
- **TUI**: [`ratatui`](https://ratatui.rs) (`--tui`) — 5 tabs (Variables/Config/Commits/Branches/Actions).
  Variable search & copy, **edit global config in the Config tab** (Enter) with the effective
  result refreshed instantly and **saved to GitVersion.yml as a minimal diff**, first-parent
  commits with the version source marked, per-branch recompute, and actions (create tag/branch,
  set next-version, **edit/run exec hooks**, save
  config, clear cache, dynamic clone, recompute). The version hook is reflected immediately.
  Panics are caught (`catch_unwind`), the terminal is restored, and it exits gracefully
- **Workflows**: GitFlow / GitHubFlow / TrunkBased (Mainline)
- **Version strategies**: ConfiguredNextVersion, TaggedCommit, MergeMessage, VersionInBranchName,
  TrackReleaseBranches, Fallback, Mainline
- **Deployment modes**: ManualDeployment / ContinuousDelivery / ContinuousDeployment
- **Output**: JSON, dot-env, build-server, single variable (`-v`), format string (`--format`)
- **Log file**: `--log`/`-l <FILE>` (upstream `/l`) appends timestamped log output to a file while
  keeping stdout clean for the version result
- **Build-agent integration**: TeamCity, Azure Pipelines, GitHub Actions, GitLab CI, Jenkins,
  AppVeyor, TravisCI, Drone, CodeBuild, ContinuaCI, EnvRun, MyGet, BitBucket, BuildKite,
  SpaceAutomation — auto-detected via environment, emitted in each CI's format (`--output build-server`)
- **File output**: update/create AssemblyInfo (`--updateassemblyinfo [file] [--ensureassemblyinfo]`),
  update project files (`--updateprojectfiles`, real XML parsing rather than regex),
  Wix version file (`--updatewixversionfile`)
- **Package manifests**: `--updatepackagefiles` updates the version in `package.json` (Node.js),
  `Cargo.toml` (Rust, incl. `[workspace.package]`), and `pyproject.toml` (Python, PEP 621/Poetry)
  using format-preserving parsers (serde_json/toml_edit). Cargo workspace members that inherit
  via `version.workspace = true` are left untouched, and internal path dependencies
  (`{ path = "…", version = "…" }`) have their version requirement bumped in lockstep
- **External command hooks (exec)**: like semantic-release's exec plugin, run shell commands in
  lifecycle hooks (`verify`/`prepare`/`publish`/`success`/`fail`). Version variables are exposed
  as `GitVersion_*` env vars and `{Variable}`/`{env:VAR}` templates. The `version` hook modifies
  the version from the command's stdout (apply next-version, then recompute). Supports
  `--exec`/`--exec-version`/`--dry-run`. As a Rust convenience, the computed version is also
  exported under the standard Cargo names (`CARGO_PKG_VERSION`,
  `CARGO_PKG_VERSION_MAJOR`/`MINOR`/`PATCH`/`PRE`) so build commands pick it up natively
- **Result caching**: results stored at `<.git>/gitversion_cache/<key>.json`. The key is a SHA1
  of refs·HEAD·config file·overrideconfig, so it auto-invalidates when repo state changes.
  Disable with `--nocache`
- **Dynamic remote repository**: `--url <repo> --branch <b>` clones and computes (`-u`/`-p` auth,
  `-c` commit, `--dynamicRepoLocation`). Pure-Rust gix clone over https/file and SSH
  (`ssh://`, scp-like `git@host:path`, using system ssh)
  - **Credential helper / OS keyring**: for https auth it speaks git's credential-helper protocol.
    Without `-u`/`-p` it invokes the configured `credential.helper`, so credentials stored in macOS
    Keychain (`osxkeychain`), GCM, libsecret, etc. are used automatically (full get/erase protocol)

## Installation

### Homebrew (macOS / Linux)

```bash
brew install zcube/tap/gitversion-rs
```

Installs the **`gitversion-rs`** command. It is deliberately named `gitversion-rs` (not
`gitversion`) so it never collides with the official .NET [GitVersion](https://gitversion.net),
which also ships a `gitversion` command — the two can be installed side by side.

### Prebuilt binary

Download the archive for your platform from the
[Releases](https://github.com/zcube/gitversion-rs/releases) page and put it on your `PATH`:

```bash
tar xzf gitversion-rs-v0.1.0-aarch64-apple-darwin.tar.gz
install -m 0755 gitversion-rs /usr/local/bin/
```

Targets: macOS (arm64/x86_64), Linux (x86_64/aarch64, gnu/musl), Windows (x86_64).

### From source

```bash
cargo install --git https://github.com/zcube/gitversion-rs --locked
# or, in a clone:
cargo build --release   # -> target/release/gitversion-rs
```

## Usage

```bash
# Print all variables of the current repo as JSON
gitversion-rs

# Single variable
gitversion-rs -v FullSemVer

# Format string
gitversion-rs --format "v{Major}.{Minor}.{Patch} ({EscapedBranchName})"

# Output formats
gitversion-rs --output json
gitversion-rs --output dot-env
gitversion-rs --output build-server

# Config / overrides
gitversion-rs --config GitVersion.yml
gitversion-rs --overrideconfig next-version=2.0.0
gitversion-rs --showconfig

# External command hooks (exec) — version variables exposed as env/templates
gitversion-rs --exec 'npm version {SemVer} --no-git-tag-version'
gitversion-rs --exec-version './scripts/decide-version.sh'
gitversion-rs --exec 'make release' --dry-run

# Interactive TUI
gitversion-rs --tui

# Language (default English)
gitversion-rs --lang ko
gitversion-rs --lang ja
gitversion-rs --lang zh

# Compute for a specific branch
gitversion-rs -b release/2.0.0
```

## Examples

The [`examples/`](./examples/) directory shows how to inject the computed version into a
Rust build:

- [`examples/build_inject.rs`](./examples/build_inject.rs) — use gitversion-rs **as a
  library** from a `build.rs`, overriding `CARGO_PKG_VERSION` via `cargo:rustc-env=`.
  Runnable: `cargo run --example build_inject`.
- [`examples/exec-config-inject/`](./examples/exec-config-inject/) — drive the build from
  a `GitVersion.yml` **exec hook**, with the version exported as `CARGO_PKG_VERSION*` /
  `GitVersion_*` environment variables.

## Configuration file

Searches `GitVersion.yml`, `GitVersion.yaml`, `.GitVersion.yml`, `.GitVersion.yaml` in the
working directory (and repo root). Keys use the same kebab-case as upstream GitVersion.

```yaml
workflow: GitFlow/v1
next-version: 1.0.0
tag-prefix: "[vV]?"
branches:
  develop:
    increment: Minor
    label: alpha
```

## Project structure

| Module | Role | Upstream counterpart |
|---|---|---|
| `src/git` | gix-based repository access | `GitVersion.LibGit2Sharp` |
| `src/config` | config model / workflow defaults / loader / effective | `GitVersion.Configuration` |
| `src/version` | SemanticVersion and calculation engine | `GitVersion.Core` |
| `src/output` | output variables / formatters | `GitVersion.Output` |
| `src/cli` | clap arguments | `GitVersion.App` |
| `src/tui` | ratatui UI | (new) |
| `src/i18n.rs` + `locales/` | rust-i18n locale handling | (new) |

> Note: `refs/gitversion` is the .NET source this port was based on; it is excluded from
> tracking via `.gitignore`.

## Testing

Uses **differential testing** with the real GitVersion 6.x binary as the golden reference.

```bash
# Full test suite (unit + fixture integration)
cargo test

# Regenerate fixtures (requires the real gitversion binary)
GITVERSION_BIN=/opt/homebrew/bin/gitversion ./tests/build_fixtures.sh
```

- `tests/build_fixtures.sh`: builds per-scenario git repos, runs real GitVersion to record the
  golden `expected.json`, then packs them into `testdata/fixtures.tar.gz`.
- `tests/fixtures.rs`: unpacks into a temp dir and compares our engine's output field-by-field
  against the golden values. No git/gitversion needed at test time (reproducible).

## Known simplifications / not implemented

- `track-merge-target`: a flag consumed only by upstream's `MainlineVersionStrategy` and
  `GetTaggedSemanticVersion()`. This port already considers all tags reachable from HEAD, so
  reachable merge-target tags are covered; unreachable ones (mainly Mainline) are not.
- `/nofetch /nonormalize /allowshallow` are recognized but are honest no-ops given this port's
  structure (dynamic clone performs fetch/normalize directly).
- `GitVersionInformation` source-file generation is handled by an MSBuild task (not the CLI) upstream,
  so it is out of scope for this CLI port.

Verification is guaranteed by differential tests against the real GitVersion 6.7.0 binary
(scenarios × output fields, 5 build agents, file output).
