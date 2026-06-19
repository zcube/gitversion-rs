# Build-time version injection via exec hooks (config-based)

Inject the GitVersion-computed version into a Rust build **without writing any Rust
glue** — just configure an `exec` hook in `GitVersion.yml` and run the CLI.

When `gitversion-rs` runs an exec command it exports the computed version into the
environment under two sets of names:

| Name | Example | Source variable |
|---|---|---|
| `CARGO_PKG_VERSION` | `1.4.0-alpha.3` | `SemVer` |
| `CARGO_PKG_VERSION_MAJOR` | `1` | `Major` |
| `CARGO_PKG_VERSION_MINOR` | `4` | `Minor` |
| `CARGO_PKG_VERSION_PATCH` | `0` | `Patch` |
| `CARGO_PKG_VERSION_PRE` | `alpha.3` | `PreReleaseTag` |
| `GitVersion_*` | `GitVersion_FullSemVer=1.4.0-alpha.3+5` | every output variable |

`{SemVer}` / `{FullSemVer}` template tokens are also substituted into the command
string itself (see [`GitVersion.yml`](./GitVersion.yml)).

## Run it

From a Rust project that contains this `GitVersion.yml`:

```bash
gitversion-rs --config GitVersion.yml
```

The `prepare` hook stamps the version into `Cargo.toml` and runs `cargo build --release`.

Dry-run to see the command without side effects:

```bash
gitversion-rs --config GitVersion.yml --dry-run
```

Or supply an ad-hoc prepare command (runs after the config's `prepare`):

```bash
gitversion-rs --exec 'docker build --build-arg VERSION=$CARGO_PKG_VERSION .'
```

## Important caveat

`cargo build` derives `CARGO_PKG_VERSION` from `Cargo.toml`, so the inherited env var
alone does **not** change the version the inner crate compiles with — you must stamp
`Cargo.toml` first (the hook does this with `sed`). The exported env vars are most
useful for *scripts* and *external tools* (Docker build-args, release uploaders, etc.).

If you want to override a crate's own version **without** modifying `Cargo.toml`, use
the library / `build.rs` approach instead — see [`../build_inject.rs`](../build_inject.rs),
where `cargo:rustc-env=CARGO_PKG_VERSION=...` is applied by Cargo *after* it reads the
manifest and therefore wins.

## Files

- [`GitVersion.yml`](./GitVersion.yml) — the exec hook configuration.
- [`build.rs`](./build.rs) — sample consumer `build.rs` that reads `CARGO_PKG_VERSION`.
