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

# Create release branch from main, bump version, commit, tag, push to origin
# Usage: just release-start        (patch)
#        just release-start minor
#        just release-start major
release-start level="patch":
    #!/usr/bin/env bash
    set -euo pipefail
    CURRENT=$(git rev-parse --abbrev-ref HEAD)
    if [[ "${CURRENT}" != "main" ]]; then
        echo "Error: must be on main branch (currently on ${CURRENT})"
        exit 1
    fi
    if git show-ref --verify refs/heads/release >/dev/null 2>&1; then
        echo "Error: release branch already exists. Use 'just release-retry' to reset it."
        exit 1
    fi
    git pull --ff-only
    git checkout -b release
    cargo release {{level}} --execute --no-publish

# Publish the tagged commit to crates.io (manual fallback)
publish:
    cargo release publish --execute

# Trigger release-publish.yml: publish GitHub release, crates.io, FF merge release->main
gh-publish:
    #!/usr/bin/env bash
    set -euo pipefail
    VERSION=$(yq -p toml -oy '.package.version' Cargo.toml)
    gh workflow run release-publish.yml -f tag="v${VERSION}"

# Reset a failed release: delete draft/tag/release branch and recreate from latest main
# Blocked if the GitHub release is published or the version is already on crates.io
# Usage: just release-retry        (patch)
#        just release-retry minor
release-retry level="patch":
    #!/usr/bin/env bash
    set -euo pipefail
    VERSION=$(yq -p toml -oy '.package.version' Cargo.toml)
    TAG="v${VERSION}"
    CRATE="gitversion-rs"
    STATUS=$(curl -s -o /dev/null -w "%{http_code}" -A "release-retry/1.0" \
        "https://crates.io/api/v1/crates/${CRATE}/${VERSION}")
    if [[ "${STATUS}" == "200" ]]; then
        echo "Error: ${CRATE} v${VERSION} is already on crates.io. Cannot retry."
        exit 1
    fi
    IS_DRAFT=$(gh release view "${TAG}" --json isDraft -q '.isDraft' 2>/dev/null || echo "none")
    if [[ "${IS_DRAFT}" == "false" ]]; then
        echo "Error: ${TAG} is already published. Cannot retry."
        exit 1
    fi
    if [[ "${IS_DRAFT}" == "true" ]]; then
        gh release delete "${TAG}" --yes --cleanup-tag
    fi
    git tag -d "${TAG}" 2>/dev/null || true
    git push origin --delete "${TAG}" 2>/dev/null || true
    git push origin --delete release 2>/dev/null || true
    git checkout main
    git pull --ff-only
    git branch -D release 2>/dev/null || true
    git checkout -b release
    cargo release {{level}} --execute --no-publish
