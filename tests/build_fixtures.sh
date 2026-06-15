#!/usr/bin/env bash
#
# 테스트 fixture 생성기.
#
# 시나리오별 git 저장소를 만들고, 실제 GitVersion 6.x 바이너리를 돌려 golden
# 기대값(expected.json)을 각 저장소에 기록한 뒤, 전부 testdata/fixtures.tar.gz
# 로 압축한다. 테스트(tests/fixtures.rs)는 이 압축만 풀어서 우리 엔진의 출력을
# golden 값과 비교하므로, 테스트 시점에는 git/gitversion 이 필요 없다.
#
# 사용법:
#   GITVERSION_BIN=/opt/homebrew/bin/gitversion ./tests/build_fixtures.sh
#
set -euo pipefail

GV="${GITVERSION_BIN:-/opt/homebrew/bin/gitversion}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/testdata/fixtures.tar.gz"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

if [ ! -x "$GV" ] && ! command -v "$GV" >/dev/null 2>&1; then
  echo "오류: GitVersion 바이너리를 찾을 수 없습니다: $GV" >&2
  exit 1
fi

echo "GitVersion: $("$GV" /version 2>/dev/null | head -1)"
mkdir -p "$ROOT/testdata"

# 결정론적 커밋: 날짜/작성자 고정 → SHA 재현 가능.
export GIT_AUTHOR_NAME=test GIT_AUTHOR_EMAIL=test@example.com
export GIT_COMMITTER_NAME=test GIT_COMMITTER_EMAIL=test@example.com
TICK=1609459200 # 2021-01-01T00:00:00Z

newrepo() { # $1 = name, $2 = initial branch
  REPO="$STAGE/$1"
  mkdir -p "$REPO"
  git -C "$REPO" init -q -b "${2:-main}"
  git -C "$REPO" config commit.gpgsign false
  git -C "$REPO" config core.hooksPath /dev/null
  CUR="$REPO"
}

commit() { # $1 = message
  TICK=$((TICK + 60))
  GIT_AUTHOR_DATE="$TICK +0000" GIT_COMMITTER_DATE="$TICK +0000" \
    git -C "$CUR" commit -q --no-verify --allow-empty -m "$1"
}

tagcommit() { # $1 = version (tag), creates a commit then tags it
  commit "release $1"
  git -C "$CUR" tag "$1"
}

branch() { git -C "$CUR" checkout -q -b "$1"; }
checkout() { git -C "$CUR" checkout -q "$1"; }
writeconfig() { printf '%s\n' "$1" > "$CUR/GitVersion.yml"; }

record() { # 실제 GitVersion 출력을 golden 으로 저장
  "$GV" "$CUR" /nocache /output json > "$CUR/expected.json" 2>/dev/null || true
  if ! grep -q '"FullSemVer"' "$CUR/expected.json" 2>/dev/null; then
    echo "  !! $(basename "$CUR"): GitVersion 출력 없음 → 시나리오 제외" >&2
    rm -rf "$CUR"
    return 0
  fi
  printf '  %-28s -> %s\n' "$(basename "$CUR")" \
    "$(grep -o '"FullSemVer": *"[^"]*"' "$CUR/expected.json" | head -1)"
}

echo "시나리오 생성 중..."

# 1. main, 3 commits, no tag
newrepo main_3commits main; commit a; commit b; commit c; record

# 2. main, tag v1.0.0 then 2 commits
newrepo main_tag_plus2 main; tagcommit v1.0.0; commit a; commit b; record

# 3. main, current commit tagged
newrepo main_current_tagged main; commit a; tagcommit v2.0.0; record

# 4. develop off main, +1 commit
newrepo develop_plus1 main; tagcommit v1.0.0; branch develop; commit a; record

# 5. develop off main, +2 commits
newrepo develop_plus2 main; tagcommit v1.0.0; branch develop; commit a; commit b; record

# 6. release branch from main (CanTakeVersionFromReleaseBranch)
newrepo release_from_main main
tagcommit 1.0.3; commit c1; commit c2; commit c3; commit c4; commit c5
branch release-2.0.0; record

# 7. feature off develop
newrepo feature_off_develop main
tagcommit v1.0.0; branch develop; commit d1
branch feature/JIRA-123; commit f1; record

# 8. +semver: minor message on main
newrepo semver_minor_main main; tagcommit v1.0.0; commit "feat
+semver: minor"; record

# 9. +semver: major message on main
newrepo semver_major_main main; tagcommit v1.0.0; commit "break
+semver: major"; record

# 10. GitHubFlow workflow via config
newrepo githubflow_main main
writeconfig 'workflow: GitHubFlow/v1'
tagcommit v1.0.0; commit a; commit b; record

# 11. next-version via config (no tags)
newrepo nextversion_config main
writeconfig 'next-version: 2.0.0'
commit a; commit b; record

# 12. hotfix branch (GitFlow)
newrepo hotfix_branch main
tagcommit v1.0.0; branch hotfix/1.0.1; commit h1; record

# 13. support branch (GitFlow)
newrepo support_branch main
tagcommit v1.0.0; commit a; branch support/1.x; commit s1; record

# 14. custom tag-prefix
newrepo tagprefix_custom main
writeconfig 'tag-prefix: "ver"'
commit init; git -C "$CUR" tag ver1.2.0; commit a; record

# 15. pre-release tag on history (tag v1.0.0-beta.1 then commits)
newrepo prerelease_tag main
commit init; git -C "$CUR" tag v1.0.0-beta.1; commit a; commit b; record

# 16. multiple tags, latest wins
newrepo multiple_tags main
tagcommit v1.0.0; commit a; tagcommit v1.1.0; commit b; commit c; record

# 17. ignore a specific sha
newrepo ignore_sha main
tagcommit v1.0.0; commit keep1
IGN=$(git -C "$CUR" rev-parse HEAD)
commit keep2; commit keep3
writeconfig "ignore:
  sha:
    - $IGN"
record

# 18. commits-before ignore (오래된 날짜 → 아무것도 제외 안 함: 파싱 안정성 확인)
newrepo ignore_before main
tagcommit v1.0.0; commit a; commit b
writeconfig 'ignore:
  commits-before: 2020-01-01T00:00:00'
record

# 19. custom commit-date-format
newrepo commitdate_format main
writeconfig 'commit-date-format: "yyyy.MM.dd"'
commit a; commit b; record

# 22. GitHubFlow release branch
newrepo githubflow_release main
writeconfig 'workflow: GitHubFlow/v1'
tagcommit v1.0.0; commit a; branch release/2.0.0; commit r1; record

# 23. next-version partial "1"
newrepo nextversion_partial main
writeconfig 'next-version: "1"'
commit a; commit b; record

# 24. feature with +semver:major
newrepo feature_semver_major main
tagcommit v1.0.0; branch develop; commit d1
branch feature/big; commit "huge
+semver: major"; record

# 25. release branch then more commits
newrepo release_plus_commits main
tagcommit 1.0.0; commit c1; commit c2
branch release-1.1.0; commit r1; commit r2; record

# 26. semantic-version-format Loose (부분 태그 1.2 수용)
newrepo semver_loose main
writeconfig 'semantic-version-format: Loose'
commit init; git -C "$CUR" tag 1.2; commit a; record

# 27. semantic-version-format Strict (부분 태그 1.2 거부 → fallback)
newrepo semver_strict main
writeconfig 'semantic-version-format: Strict'
commit init; git -C "$CUR" tag 1.2; commit a; record

# 28. assembly 커스텀 포맷
newrepo assembly_format main
writeconfig "assembly-versioning-format: '{Major}.{Minor}.{Patch}.{WeightedPreReleaseNumber}'
assembly-informational-format: '{Major}.{Minor}'"
tagcommit v1.0.0; commit a; record

# 29. feature off main (develop 없음 → main Patch 상속)
newrepo feature_off_main main
tagcommit v1.0.0; branch feature/foo; commit f1; record

# 30. 안정 릴리스의 WeightedPreReleaseNumber = tag-pre-release-weight(60000)
newrepo stable_weighted main
commit a; tagcommit v3.0.0; record

# 31~33. Mainline 전략(per-commit 누적)
MAINLINE_CFG='strategies:
- Mainline
mode: ContinuousDeployment'
newrepo mainline_3commits main
writeconfig "$MAINLINE_CFG"
commit a; commit b; commit c; record

newrepo mainline_tag main
writeconfig "$MAINLINE_CFG"
tagcommit v1.0.0; commit a; commit b; record

newrepo mainline_minor main
writeconfig "$MAINLINE_CFG"
commit a; commit "feat
+semver: minor"; commit c; record

# 34. 빌드에이전트 golden: 동일 저장소에 대해 각 CI 의 실제 출력을 저장.
newrepo buildagent_repo main
tagcommit v1.0.0; commit b
record   # expected.json (일반 JSON)
git -C "$CUR" remote add origin https://example.com/r.git
gen_agent(){ # $1=AgentName  $2=env assignments  $3=grep filter
  env $2 "$GV" "$CUR" /nocache /nonormalize /output buildserver 2>/dev/null \
    | grep -E "$3" > "$CUR/agent_$1.txt" || true
  printf '  buildagent/%-16s -> %s줄\n' "$1" "$(wc -l < "$CUR/agent_$1.txt" | tr -d ' ')"
}
gen_agent TeamCity       "TEAMCITY_VERSION=2020 Git_Branch=main" '^##teamcity|^Set '
gen_agent AzurePipelines "TF_BUILD=True"                          '^##vso|^Set '
gen_agent ContinuaCi     "ContinuaCI.Version=1"                   '^@@continua|^Set '
gen_agent MyGet          "BuildRunner=MyGet"                      '^##myget|^Set '
gen_agent Drone          "DRONE=true"                             '^GitVersion_|^Set '

echo "압축: $OUT"
tar -C "$STAGE" -czf "$OUT" .
echo "완료. 시나리오 수: $(ls "$STAGE" | wc -l | tr -d ' ')"
