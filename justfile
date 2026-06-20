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
    cargo release {{level}} --execute --no-publish

# Publish the tagged commit to crates.io
# Run after `just bump` has pushed the tag
publish:
    cargo release publish --execute

# Trigger release-publish.yml workflow to publish draft release and push to crates.io
gh-publish:
    #!/usr/bin/env bash
    set -euo pipefail
    VERSION=$(yq -p toml -oy '.package.version' Cargo.toml)
    gh workflow run release-publish.yml -f tag="v${VERSION}"

# Delete existing draft release + tag, re-tag HEAD, push to re-trigger CI
# Refuses to run if the release is already published (non-draft) or on crates.io
gh-retag:
    #!/usr/bin/env bash
    set -euo pipefail
    VERSION=$(yq -p toml -oy '.package.version' Cargo.toml)
    TAG="v${VERSION}"
    CRATE="gitversion-rs"
    STATUS=$(curl -s -o /dev/null -w "%{http_code}" -A "gh-retag/1.0" \
        "https://crates.io/api/v1/crates/${CRATE}/${VERSION}")
    if [[ "${STATUS}" == "200" ]]; then
        echo "Error: ${CRATE} v${VERSION} is already published to crates.io. Refusing to retag."
        exit 1
    fi
    IS_DRAFT=$(gh release view "${TAG}" --json isDraft -q '.isDraft' 2>/dev/null || echo "none")
    if [[ "${IS_DRAFT}" == "false" ]]; then
        echo "Error: ${TAG} is not a draft release. Refusing to retag."
        exit 1
    fi
    echo "Re-tagging ${TAG} at HEAD"
    if [[ "${IS_DRAFT}" == "true" ]]; then
        gh release delete "${TAG}" --yes --cleanup-tag
    fi
    git tag -d "${TAG}" 2>/dev/null || true
    git push origin --delete "${TAG}" 2>/dev/null || true
    git tag -a "${TAG}" -m "release: ${TAG#v}"
    git push origin "${TAG}"
