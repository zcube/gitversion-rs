# Examples

Build-time **version injection** with gitversion-rs — computing a semantic version
from Git history and feeding it into a Rust build. Two approaches:

| Example | Approach | When to use |
|---|---|---|
| [`build_inject.rs`](./build_inject.rs) | **Library** — import `gitversion-rs` in `build.rs` | You control the project's `build.rs` and want to override `CARGO_PKG_VERSION` without touching `Cargo.toml`. |
| [`exec-config-inject/`](./exec-config-inject/) | **Exec hook** — configure `GitVersion.yml`, run the CLI | No Rust glue; drive the build (and side effects like Docker/publish) from config. |

In both cases the version is exposed under the standard Cargo names
(`CARGO_PKG_VERSION`, `CARGO_PKG_VERSION_MAJOR/MINOR/PATCH/PRE`) as well as the richer
`GitVersion_*` / `GITVERSION_*` variables.

## 1. Library approach (runnable)

`examples/build_inject.rs` is a real Cargo example. Run it from the repo root to see
the `cargo:rustc-env=` lines a `build.rs` would emit:

```bash
cargo run --example build_inject
```

In a downstream project, add gitversion-rs as a build-dependency and move that logic
into `build.rs`. See the file's doc comment for the full snippet.

## 2. Exec-hook approach (config-based)

See [`exec-config-inject/`](./exec-config-inject/). Configure an `exec` hook and run:

```bash
gitversion-rs --config GitVersion.yml
```

The CLI computes the version, exports it into the environment, and runs your hook.
