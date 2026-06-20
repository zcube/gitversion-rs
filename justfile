# Show the version gitversion-rs computes for the current commit
version:
    gitversion-rs -v FullSemVer

# Build with CARGO_PKG_VERSION_PRE injected from gitversion-rs
build:
    gitversion-rs --exec "cargo build --release"

# Build and install to ~/.cargo/bin with CARGO_PKG_VERSION_PRE injected
install:
    gitversion-rs --exec "cargo install --path . --locked"

# Dry-run: show what cargo-release would do without making changes
check level="patch":
    cargo release {{level}}

# Bump version, commit Cargo.toml, create annotated tag, push
# Usage: just bump        (patch)
#        just bump minor
#        just bump major
bump level="patch":
    cargo release {{level}} --execute

# Publish the tagged commit to crates.io
# Run after `just bump` has pushed the tag
publish:
    cargo release publish --execute

# Publish the latest GitHub draft release (make it public)
gh-publish:
    gh release edit $(git describe --tags --abbrev=0) --draft=false

# Delete existing draft release + tag, re-tag HEAD, push to re-trigger CI
gh-retag:
    #!/usr/bin/env bash
    set -euo pipefail
    TAG=$(git describe --tags --abbrev=0)
    echo "Re-tagging ${TAG} at HEAD"
    gh release delete "${TAG}" --yes --cleanup-tag 2>/dev/null || true
    git tag -d "${TAG}" 2>/dev/null || true
    git tag -a "${TAG}" -m "release: ${TAG#v}"
    git push origin "${TAG}"
