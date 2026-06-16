# gitversion (Rust port)

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
  set next-version, **toggle Conventional Commits (persisted)**, **edit/run exec hooks**, save
  config, clear cache, dynamic clone, recompute). The version hook is reflected immediately.
  Panics are caught (`catch_unwind`), the terminal is restored, and it exits gracefully
- **Workflows**: GitFlow / GitHubFlow / TrunkBased (Mainline)
- **Version strategies**: ConfiguredNextVersion, TaggedCommit, MergeMessage, VersionInBranchName,
  TrackReleaseBranches, Fallback, Mainline
- **Increment conventions**: GitVersion `+semver:` and **Conventional Commits** (`feat`→minor,
  `fix`/`perf`→patch, `feat!`/`BREAKING CHANGE:`→major), selectable via
  `commit-message-convention: ConventionalCommits` (borrowed from a semantic-release review)
- **Deployment modes**: ManualDeployment / ContinuousDelivery / ContinuousDeployment
- **Output**: JSON, dot-env, build-server, single variable (`-v`), format string (`--format`)
- **Build-agent integration**: TeamCity, Azure Pipelines, GitHub Actions, GitLab CI, Jenkins,
  AppVeyor, TravisCI, Drone, CodeBuild, ContinuaCI, EnvRun, MyGet, BitBucket, BuildKite,
  SpaceAutomation — auto-detected via environment, emitted in each CI's format (`--output build-server`)
- **File output**: update/create AssemblyInfo (`--updateassemblyinfo [file] [--ensureassemblyinfo]`),
  update project files (`--updateprojectfiles`, real XML parsing rather than regex),
  Wix version file (`--updatewixversionfile`)
- **Package manifests**: `--updatepackagefiles` updates the version in `package.json` (Node.js),
  `Cargo.toml` (Rust), and `pyproject.toml` (Python, PEP 621/Poetry) using format-preserving
  parsers (serde_json/toml_edit)
- **External command hooks (exec)**: like semantic-release's exec plugin, run shell commands in
  lifecycle hooks (`verify`/`prepare`/`publish`/`success`/`fail`). Version variables are exposed
  as `GitVersion_*` env vars and `{Variable}`/`{env:VAR}` templates. The `version` hook modifies
  the version from the command's stdout (apply next-version, then recompute). Supports
  `--exec`/`--exec-version`/`--dry-run`
- **Result caching**: results stored at `<.git>/gitversion_cache/<key>.json`. The key is a SHA1
  of refs·HEAD·config file·overrideconfig, so it auto-invalidates when repo state changes.
  Disable with `--nocache`
- **Dynamic remote repository**: `--url <repo> --branch <b>` clones and computes (`-u`/`-p` auth,
  `-c` commit, `--dynamicRepoLocation`). Pure-Rust gix clone over https/file and SSH
  (`ssh://`, scp-like `git@host:path`, using system ssh)
  - **Credential helper / OS keyring**: for https auth it speaks git's credential-helper protocol.
    Without `-u`/`-p` it invokes the configured `credential.helper`, so credentials stored in macOS
    Keychain (`osxkeychain`), GCM, libsecret, etc. are used automatically (full get/erase protocol)

## Build

```bash
cargo build --release
```

## Usage

```bash
# Print all variables of the current repo as JSON
gitversion

# Single variable
gitversion -v FullSemVer

# Format string
gitversion --format "v{Major}.{Minor}.{Patch} ({EscapedBranchName})"

# Output formats
gitversion --output json
gitversion --output dot-env
gitversion --output build-server

# Config / overrides
gitversion --config GitVersion.yml
gitversion --overrideconfig next-version=2.0.0
gitversion --showconfig

# External command hooks (exec) — version variables exposed as env/templates
gitversion --exec 'npm version {SemVer} --no-git-tag-version'
gitversion --exec-version './scripts/decide-version.sh'
gitversion --exec 'make release' --dry-run

# Interactive TUI
gitversion --tui

# Language (default English)
gitversion --lang ko
gitversion --lang ja
gitversion --lang zh

# Compute for a specific branch
gitversion -b release/2.0.0
```

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
- Log file output (`/l`) is not implemented. `/nofetch /nonormalize /allowshallow` are recognized
  but are honest no-ops given this port's structure (dynamic clone performs fetch/normalize directly).
- `GitVersionInformation` source-file generation is handled by an MSBuild task (not the CLI) upstream,
  so it is out of scope for this CLI port.

Verification is guaranteed by differential tests against the real GitVersion 6.7.0 binary
(scenarios × output fields, 5 build agents, file output).
