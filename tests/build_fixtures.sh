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

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/testdata/fixtures.tar.gz"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

# golden 값은 반드시 .NET GitVersion(원본)으로 생성해야 한다. 우리 Rust 포트가
# PATH 의 `gitversion` 을 가릴 수 있으므로(brew/cargo install), 후보를 순회하며
# ".NET GitVersion 인지" 검증해 첫 번째로 통과하는 것을 쓴다.
#
# 검증: 슬래시 인자 `/version` 에 .NET GitVersion 의 깨끗한 semver(>=5.x)를
# 출력하는가. 우리 Rust 포트는 `/version` 을 경로로 해석해 실패하므로 자동 배제된다.
is_dotnet_gitversion() {
  local out
  out="$("$1" /version 2>/dev/null | head -1 | tr -d '[:space:]')" || return 1
  printf '%s' "$out" | grep -qE '^([5-9]|[1-9][0-9]+)\.[0-9]+\.[0-9]+'
}

find_gitversion() {
  local c
  for c in \
    "${GITVERSION_BIN:-}" \
    dotnet-gitversion \
    /opt/homebrew/bin/gitversion \
    "$(command -v gitversion 2>/dev/null || true)" \
    /usr/local/bin/gitversion; do
    [ -n "$c" ] || continue
    if command -v "$c" >/dev/null 2>&1 || [ -x "$c" ]; then
      if is_dotnet_gitversion "$c"; then
        command -v "$c" 2>/dev/null || printf '%s\n' "$c"
        return 0
      fi
    fi
  done
  return 1
}

GV="$(find_gitversion || true)"
if [ -z "$GV" ]; then
  {
    echo "오류: .NET GitVersion(원본) 바이너리를 찾을 수 없습니다."
    echo "  golden 값은 원본으로만 생성해야 합니다(우리 Rust 포트로는 자가 비교가 됩니다)."
    echo "  설치: brew install gitversion  또는  dotnet tool install -g GitVersion.Tool"
    echo "  또는: GITVERSION_BIN=<.NET gitversion 경로> $0"
  } >&2
  exit 1
fi

echo "GitVersion(.NET): $GV -> $("$GV" /version 2>/dev/null | head -1)"
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
  git -C "$REPO" config tag.gpgsign false
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
merge() { # $1=branch to merge, $2=message
  TICK=$((TICK + 60))
  GIT_AUTHOR_DATE="$TICK +0000" GIT_COMMITTER_DATE="$TICK +0000" \
    git -C "$CUR" merge --no-ff --no-verify -q "$1" -m "$2"
}
writeconfig() { printf '%s\n' "$1" > "$CUR/GitVersion.yml"; }
writefile() { # $1=상대경로  $2=내용
  mkdir -p "$CUR/$(dirname "$1")"
  printf '%s\n' "$2" > "$CUR/$1"
  git -C "$CUR" add "$1"
}
commitfile() { # $1=메시지 (staged 변경사항 커밋)
  TICK=$((TICK + 60))
  GIT_AUTHOR_DATE="$TICK +0000" GIT_COMMITTER_DATE="$TICK +0000" \
    git -C "$CUR" commit -q --no-verify -m "$1"
}

record() { # 실제 GitVersion 출력을 golden 으로 저장
  # /nocache: 디스크 캐시 미사용. /nonormalize: GitVersion 이 저장소(브랜치/refs)를
  # 수정하지 못하게 해, golden repo 가 .NET 부수효과로 오염되는 것을 원천 차단한다.
  "$GV" "$CUR" /nocache /nonormalize /output json > "$CUR/expected.json" 2>/dev/null || true
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

# release/feature 를 main 에 병합(merge 메시지 전략)
newrepo merge_release main
git -C "$CUR" config merge.ff false
tagcommit v1.0.0; branch release/2.0.0; commit r1
checkout main; merge release/2.0.0 "Merge branch 'release/2.0.0' into main"; record

newrepo merge_pr main
git -C "$CUR" config merge.ff false
tagcommit v1.0.0; branch feature/login; commit f1
checkout main; merge feature/login "Merge pull request #42 from org/feature/login"; record

# support 브랜치 병합은 release 가 아니므로 버전 무시(merge 게이팅 검증)
newrepo merge_support_ignored main
git -C "$CUR" config merge.ff false
tagcommit v1.0.0; branch support/2.0; commit s1
checkout main; merge support/2.0 "Merge remote-tracking branch 'support/2.0'"; record

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

# Mainline + feature merge: 병합 브랜치 증분을 1회로 consolidate
newrepo mainline_merge main
writeconfig "$MAINLINE_CFG"
git -C "$CUR" config merge.ff false
commit m1; branch feature/x; commit f1; commit f2
checkout main; merge feature/x "Merge branch 'feature/x'"; record

# Mainline 극단: 중간 stable 태그
newrepo mainline_midtag main
writeconfig "$MAINLINE_CFG"
commit a; commit b; git -C "$CUR" tag v2.0.0; commit c; record

# Mainline 극단: pre-release 태그 가진 feature 병합(확정)
newrepo mainline_pretag_merge main
writeconfig "$MAINLINE_CFG"
git -C "$CUR" config merge.ff false
commit m1; branch feature/y; commit f1; git -C "$CUR" tag v2.0.0-alpha.1
checkout main; merge feature/y "Merge branch 'feature/y'"; record

# Mainline + ContinuousDelivery 모드: pre-release 번호 = distance
newrepo mainline_cd main
writeconfig 'strategies:
- Mainline
mode: ContinuousDelivery'
commit a; commit b; commit c; record

# Mainline + ManualDeployment 모드
newrepo mainline_manual main
writeconfig 'strategies:
- Mainline
mode: ManualDeployment'
commit a; git -C "$CUR" tag v1.0.0; commit b; record

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

# ignore.paths: docs/ 만 건드리는 커밋은 버전 계산에서 제외
newrepo ignore_paths main
tagcommit v1.0.0
writefile docs/readme.md "doc content"; commitfile "update docs"
writefile src/main.rs "fn main(){}"; commitfile "add code"
writeconfig 'ignore:
  paths:
    - docs'
record

# GitHubFlow feature + HEAD 태그(when-current-commit-tagged: false)
# → feature HEAD 에 태그가 있어도 prevent-increment 하지 않음
newrepo githubflow_feature_tagged main
writeconfig 'workflow: GitHubFlow/v1'
tagcommit v1.0.0; branch feature/foo; commit f1
git -C "$CUR" tag v1.1.0
record

# GitHubFlow release branch: prevent-increment of-merged-branch=true, when-branch-merged=false
newrepo githubflow_release_prevent main
writeconfig 'workflow: GitHubFlow/v1'
tagcommit v1.0.0; commit a; commit b
branch release/2.0.0; commit r1; commit r2
record

# TrunkBased feature + HEAD 태그(when-current-commit-tagged: false)
newrepo trunkbased_feature_tagged main
writeconfig 'workflow: TrunkBased/preview1'
tagcommit v1.0.0; branch feature/foo; commit f1
git -C "$CUR" tag v2.0.0
record

# TrunkBased hotfix + HEAD 태그(when-current-commit-tagged: false)
newrepo trunkbased_hotfix_tagged main
writeconfig 'workflow: TrunkBased/preview1'
tagcommit v1.0.0; branch hotfix/1.0.1; commit h1
git -C "$CUR" tag v1.0.1
record

# TrunkBased main: feature 병합 후 버전 (prevent_increment.of_merged_branch=true 검증)
# main 이 of_merged_branch=true 이므로 feature 의 Minor 증분이 아닌
# main 의 Patch 증분을 사용해야 한다.
newrepo trunkbased_main_merged_feature main
writeconfig 'workflow: TrunkBased/preview1'
git -C "$CUR" config merge.ff false
tagcommit v1.0.0; commit m1
branch feature/new; commit f1; commit f2
checkout main; merge feature/new "Merge branch 'feature/new'"
record

# TrunkBased unknown 브랜치 + HEAD 태그(when-current-commit-tagged=false 검증)
# unknown 브랜치는 when_current_commit_tagged=false 이므로
# release 태그된 HEAD 커밋에서도 한 단계 더 증분해야 한다.
newrepo trunkbased_unknown_tagged main
writeconfig 'workflow: TrunkBased/preview1'
tagcommit v1.0.0
branch custom/work; commit w1
git -C "$CUR" tag v1.1.0
record

# ─── 긴 커밋 체인 / 다양한 깊이 / 다양한 메시지 시나리오 ──────────────────

# 태그 없이 12개 커밋: VersionSourceDistance=12 기준점
newrepo main_long_no_tag main
for i in $(seq 1 12); do commit "c$i"; done
record

# v1.0.0 이후 10개 커밋: VersionSourceDistance=10 검증
newrepo main_deep_after_tag main
tagcommit v1.0.0
for i in $(seq 1 10); do commit "c$i"; done
record

# 두 태그 사이 긴 거리: v1.1.0 이후 8개 커밋 → 가장 가까운 태그 선택 검증
newrepo multi_tag_deep main
tagcommit v1.0.0
for i in $(seq 1 5); do commit "a$i"; done
tagcommit v1.1.0
for i in $(seq 1 8); do commit "b$i"; done
record

# develop: 8개 커밋 (PreReleaseNumber=8 검증)
newrepo develop_8commits main
tagcommit v1.0.0
branch develop
for i in $(seq 1 8); do commit "d$i"; done
record

# develop 5개 후 feature 7개: 깊은 기능 브랜치의 거리 계산
newrepo feature_deep_develop main
tagcommit v1.0.0
branch develop
for i in $(seq 1 5); do commit "d$i"; done
branch feature/deep
for i in $(seq 1 7); do commit "f$i"; done
record

# main 3개 후 hotfix 6개: 긴 핫픽스 브랜치
newrepo hotfix_deep main
tagcommit v1.0.0
for i in $(seq 1 3); do commit "m$i"; done
branch hotfix/1.0.1
for i in $(seq 1 6); do commit "h$i"; done
record

# main 4개 후 release 7개: 긴 릴리스 브랜치
newrepo release_deep main
tagcommit v1.0.0
for i in $(seq 1 4); do commit "m$i"; done
branch release-1.1.0
for i in $(seq 1 7); do commit "r$i"; done
record

# 긴 체인 중간에 +semver:minor 혼재 (main)
newrepo main_long_with_minor main
tagcommit v1.0.0
commit c1; commit c2; commit c3
commit "feat
+semver: minor"
commit c5; commit c6; commit c7; commit c8
record

# 긴 체인에서 +semver:major가 중간에 위치
newrepo main_long_with_major main
tagcommit v1.0.0
commit c1; commit c2
commit "BREAKING CHANGE: api redesign
+semver: major"
commit c4; commit c5; commit c6; commit c7; commit c8; commit c9
record

# 여러 +semver 메시지가 긴 체인에 혼재 (가장 높은 증분이 우선)
newrepo main_mixed_semver main
tagcommit v1.0.0
commit c1
commit "fix: small patch
+semver: patch"
commit c3
commit "feat: new api
+semver: minor"
commit c5
commit "chore: cleanup"; commit c7; commit c8
record

# develop에 +semver 메시지 혼재
newrepo develop_with_semver main
tagcommit v1.0.0
branch develop
commit d1; commit d2
commit "feat
+semver: minor"
commit d4; commit d5; commit d6
record

# GitHubFlow: v1.0.0 이후 9개 커밋
newrepo githubflow_deep_main main
writeconfig 'workflow: GitHubFlow/v1'
tagcommit v1.0.0
for i in $(seq 1 9); do commit "m$i"; done
record

# GitHubFlow: main 4개 후 feature 8개
newrepo githubflow_deep_feature main
writeconfig 'workflow: GitHubFlow/v1'
tagcommit v1.0.0
for i in $(seq 1 4); do commit "m$i"; done
branch feature/big
for i in $(seq 1 8); do commit "f$i"; done
record

# GitHubFlow: feature에 +semver:minor 포함, 긴 체인
newrepo githubflow_feature_semver main
writeconfig 'workflow: GitHubFlow/v1'
tagcommit v1.0.0
commit m1; commit m2
branch feature/api
commit f1; commit f2
commit "feat: add new endpoint
+semver: minor"
commit f4; commit f5; commit f6
record

# GitHubFlow: 두 PR 순차 병합 (각 PR 여러 커밋)
newrepo githubflow_multi_pr main
writeconfig 'workflow: GitHubFlow/v1'
git -C "$CUR" config merge.ff false
tagcommit v1.0.0
branch feature/pr1; commit f1; commit f2; commit f3
checkout main; merge feature/pr1 "Merge pull request #1 from org/feature/pr1"
branch feature/pr2; commit g1; commit g2; commit g3; commit g4
checkout main; merge feature/pr2 "Merge pull request #2 from org/feature/pr2"
record

# TrunkBased: v1.0.0 이후 8개 커밋
newrepo trunkbased_long main
writeconfig 'workflow: TrunkBased/preview1'
tagcommit v1.0.0
for i in $(seq 1 8); do commit "m$i"; done
record

# TrunkBased: feature 깊은 브랜치 (5개)
newrepo trunkbased_deep_feature main
writeconfig 'workflow: TrunkBased/preview1'
tagcommit v1.0.0
commit m1; commit m2
branch feature/deep
for i in $(seq 1 5); do commit "f$i"; done
record

# Mainline: 10개 커밋, 혼합 +semver 메시지
newrepo mainline_long main
writeconfig 'strategies:
- Mainline
mode: ContinuousDeployment'
commit c1; commit c2
commit "feat
+semver: minor"
commit c4; commit c5
commit "break
+semver: major"
commit c7; commit c8; commit c9; commit c10
record

# Mainline: 태그 이후 다양한 +semver 메시지 혼재
newrepo mainline_tag_mixed main
writeconfig 'strategies:
- Mainline
mode: ContinuousDeployment'
tagcommit v1.0.0
commit a
commit "feat
+semver: minor"
commit b; commit c
commit "fix
+semver: patch"
commit d; commit e; commit f
record

# Mainline + feature 8개 커밋 병합
newrepo mainline_deep_feature main
writeconfig 'strategies:
- Mainline
mode: ContinuousDeployment'
git -C "$CUR" config merge.ff false
tagcommit v1.0.0
commit m1; commit m2
branch feature/long
for i in $(seq 1 8); do commit "f$i"; done
checkout main; merge feature/long "Merge branch 'feature/long'"
record

# 완전한 GitFlow 사이클: develop 4, feature 5, release 3
newrepo gitflow_full_cycle main
tagcommit v1.0.0
branch develop
for i in $(seq 1 4); do commit "d$i"; done
branch feature/x
for i in $(seq 1 5); do commit "fx$i"; done
checkout develop
merge feature/x "Merge branch 'feature/x' into develop"
branch release-2.0.0
for i in $(seq 1 3); do commit "r$i"; done
record

# develop에 여러 feature 병합: 각 feature 3~4개 커밋
newrepo develop_multi_feature main
git -C "$CUR" config merge.ff false
tagcommit v1.0.0
branch develop
commit d1
branch feature/a; commit a1; commit a2; commit a3
checkout develop; merge feature/a "Merge branch 'feature/a' into develop"
branch feature/b; commit b1; commit b2; commit b3; commit b4
checkout develop; merge feature/b "Merge branch 'feature/b' into develop"
record

# 여러 릴리스 사이클: v1.0.0→ 커밋 → v1.1.0 → 커밋 → v2.0.0 → 긴 체인
newrepo multi_release_cycle main
tagcommit v1.0.0
commit a1; commit a2; commit a3
tagcommit v1.1.0
commit b1; commit b2
tagcommit v2.0.0
for i in $(seq 1 7); do commit "c$i"; done
record

# support 브랜치에서 긴 체인: 패치 버전 누적
newrepo support_deep main
tagcommit v1.0.0
commit m1; commit m2; commit m3
branch support/1.x
for i in $(seq 1 6); do commit "s$i"; done
record

# ─── 중복 태그 / next-version 충돌 / tag-prefix 혼재 시나리오 ───────────────

# 동일 커밋에 릴리스 태그 2개: 최고 버전(v2.0.0)이 선택되어야 함
newrepo dual_release_tag main
commit init
git -C "$CUR" tag v1.0.0
git -C "$CUR" tag v2.0.0
commit a
record

# 동일 커밋에 pre-release 태그와 릴리스 태그 공존(HEAD)
# → 릴리스 태그(v1.0.0)가 pre-release(v1.0.0-beta.1)보다 우선해야 함
newrepo prerelease_release_same_commit main
commit init
git -C "$CUR" tag v1.0.0-beta.1
git -C "$CUR" tag v1.0.0
record

# next-version(0.5.0) < 히스토리 태그(v1.5.0): 태그+증분이 이겨야 함
newrepo tag_beats_nextversion main
writeconfig 'next-version: "0.5.0"'
tagcommit v1.5.0; commit a; commit b
record

# next-version(3.0.0) > 히스토리 태그(v1.5.0): next-version이 이겨야 함
newrepo nextversion_beats_tag main
writeconfig 'next-version: "3.0.0"'
tagcommit v1.5.0; commit a; commit b
record

# 커스텀 tag-prefix("ver") 환경에서 기본 prefix 태그(v1.0.0)와 커스텀 태그(ver2.0.0) 혼재:
# v1.0.0은 파싱 실패(무시), ver2.0.0만 활성 버전으로 인식되어야 함
newrepo tagprefix_mixed main
writeconfig 'tag-prefix: "ver"'
commit init
git -C "$CUR" tag v1.0.0
commit a
git -C "$CUR" tag ver2.0.0
commit b
record

# ─── assembly-versioning-scheme 4종 / TrackRelease / prevent-increment ─────

# assembly-versioning-scheme: Major (AssemblyVersion = Major.0.0.0)
newrepo assembly_scheme_major main
writeconfig "assembly-versioning-scheme: Major"
tagcommit v1.2.3; commit a
record

# assembly-versioning-scheme: MajorMinor (AssemblyVersion = Major.Minor.0.0)
newrepo assembly_scheme_majorminor main
writeconfig "assembly-versioning-scheme: MajorMinor"
tagcommit v1.2.3; commit a
record

# assembly-versioning-scheme: MajorMinorPatchTag (AssemblyVersion = Major.Minor.Patch.PreReleaseNumber)
newrepo assembly_scheme_patch_tag main
writeconfig "assembly-versioning-scheme: MajorMinorPatchTag"
tagcommit v1.2.3; commit a
record

# assembly-versioning-scheme: None (AssemblyVersion = 빈 문자열)
newrepo assembly_scheme_none main
writeconfig "assembly-versioning-scheme: None"
tagcommit v1.2.3; commit a
record

# develop 에서 release 브랜치 추적 (TrackReleaseBranches 전략)
# GitFlow develop 은 tracks-release-branches: true 이므로 release 브랜치 버전을 추적
newrepo track_release_develop main
tagcommit v1.0.0
branch develop
for i in $(seq 1 3); do commit "d$i"; done
checkout main
branch release-2.0.0
for i in $(seq 1 2); do commit "r$i"; done
checkout develop
record

# prevent-increment.of-merged-branch: true 가 있는 GitFlow merge
newrepo prevent_increment_merged main
git -C "$CUR" config merge.ff false
writeconfig "branches:
  release:
    prevent-increment:
      of-merged-branch: true"
tagcommit v1.0.0
branch release-2.0.0; commit r1; commit r2
checkout main
merge release-2.0.0 "Merge branch 'release-2.0.0' into main"
record

# ─── 갭 검증: label 매칭 / CD 번호 누적 / 브랜치명 sanitize / semanticVersionThreshold ────

# label 불일치 pre-release 태그: develop(alpha)에 beta 태그가 있을 때
# GetBranchSpecificTag 갭 확인 — 원본이 beta 태그를 무시하고 alpha 기반으로 계산하는지
newrepo label_mismatch_prerelease main
tagcommit v1.0.0
branch develop
commit d1
git -C "$CUR" tag v2.0.0-beta.1   # beta label, develop label=alpha → 불일치
commit d2; commit d3
record

# ContinuousDelivery + 기존 높은 pre-release 번호 태그: alpha.5 이후 3커밋
# → CD 모드에서 pre-release 번호가 alpha.8이 되어야 하는지 검증
newrepo cd_prerelease_numbered_tag main
commit init
git -C "$CUR" tag v1.0.0-alpha.5
commit a; commit b; commit c
record

# 브랜치명에 점(.)이 포함된 경우: InformationalVersion의 Branch sanitize 확인
newrepo branch_dot_in_name main
tagcommit v1.0.0
branch feature/1.0-fix
commit f1; commit f2
record

# semantic-version-threshold 설정 키 존재 여부 검증:
# v0.5.0 태그 후 커밋, threshold=1.0.0 → 원본이 threshold 로 낮은 태그를 필터하면
# Fallback 기반 결과, 무시하면 v0.5.0 기반 결과
newrepo semantic_threshold main
writeconfig 'semantic-version-threshold: "1.0.0"'
commit init
git -C "$CUR" tag v0.5.0
commit a; commit b
record

# Mainline: when_branch_merged=true 브랜치 병합 → 브랜치 설정 증분(Minor) 미적용 검증
# feature 에 prevent-increment.when-branch-merged=true 를 주면
# 병합되어도 Minor 가 아닌 Patch(main 기본값)만 적용되어야 한다.
newrepo mainline_when_branch_merged main
writeconfig 'workflow: TrunkBased/preview1
branches:
  feature:
    prevent-increment:
      when-branch-merged: true'
git -C "$CUR" config merge.ff false
tagcommit v1.0.0; commit m1
branch feature/skip; commit f1; commit f2
checkout main; merge feature/skip "Merge branch 'feature/skip'"
record

# ConfiguredNextVersion: pre-release label 불일치 시 건너뜀 검증
# develop(label=alpha)에서 next-version="2.0.0-beta"는 label 불일치라 무시되어야 함.
newrepo nextversion_label_mismatch main
writeconfig 'next-version: "2.0.0-beta"'
tagcommit v1.0.0
branch develop; commit d1; commit d2
record

# numeric-only pre-release 태그(promote 검증): v1.0.0-1 은 pre-release 로 인식되어야 함.
# promote_tag_even_if_name_is_empty=true 이면 has_tag()=true → pre-release 태그로 처리.
newrepo numeric_prerelease_tag main
commit a
git -C "$CUR" tag v1.0.0-1
commit b
record

# BitBucketPullv7 머지 메시지: "Pull request #N\n\nMerge in X from Y to Z" 멀티라인 포맷.
# SourceBranch=release/2.0.0 에서 버전 2.0.0 을 추출해야 한다(MergeMessage 전략).
newrepo merge_bitbucket_v7 main
tagcommit v1.0.0
branch release/2.0.0; commit r1; commit r2
checkout main
merge release/2.0.0 "$(printf 'Pull request #7: Release 2.0.0\n\nMerge in MYPROJ/myrepo from release/2.0.0 to main')"
record

# 4-part 태그(v1.2.3.4) + 기본 Strict 포맷: Strict 는 4-part 를 버전으로 인식하지 않아
# 태그가 버려지고 fallback(0.0.x) 이 된다(원본 ParseStrict).
newrepo loose_four_part_tag main
commit init
git -C "$CUR" tag v1.2.3.4
commit a; commit b
record

# semantic-version-format: Loose + 4-part 태그(v1.2.3.4): Strict 와 달리 코어 1.2.3 인식.
newrepo loose_format_four_part main
writeconfig 'semantic-version-format: Loose'
commit init
git -C "$CUR" tag v1.2.3.4
commit a; commit b
record

# semantic-version-format: Loose + 부분 버전 태그(v1.2): 1.2.0 으로 인식되어야 한다.
newrepo loose_format_partial_tag main
writeconfig 'semantic-version-format: Loose'
commit init
git -C "$CUR" tag v1.2
commit a; commit b
record

# 기본 Strict + 부분 버전 태그(v1.2): Strict 는 부분 버전을 거부해 태그가 버려진다.
newrepo strict_partial_tag main
commit init
git -C "$CUR" tag v1.2
commit a; commit b
record

# semantic-version-format: Loose + 부분 버전 브랜치명(release/1.3): 1.3.0 추출.
newrepo loose_format_branch main
writeconfig 'semantic-version-format: Loose'
tagcommit v1.0.0
branch release/1.3; commit r1
record

# commit-message-incrementing: Disabled - +semver 메시지를 무시하고 기본 증분만 적용.
newrepo cfg_msg_inc_disabled main
writeconfig 'commit-message-incrementing: Disabled'
tagcommit v1.0.0
commit "feat
+semver: major"
record

# commit-message-incrementing: MergeMessageOnly - 일반 커밋의 +semver 는 무시.
newrepo cfg_msg_inc_mergeonly main
writeconfig 'commit-message-incrementing: MergeMessageOnly'
tagcommit v1.0.0
commit "feat
+semver: minor"
record

# increment: Major (전역) - 기본 증분 필드를 Major 로. 태그 후 커밋은 2.0.0.
newrepo cfg_increment_major main
writeconfig 'increment: Major'
tagcommit v1.0.0
commit a
record

# increment: None (전역) - 증분하지 않음. 태그 후 커밋도 코어 동일.
newrepo cfg_increment_none main
writeconfig 'increment: None'
tagcommit v1.0.0
commit a
record

# branches.main.increment: Major - 브랜치별 직접 increment(전역과 달리 실제 적용).
newrepo cfg_branch_increment_major main
writeconfig 'branches:
  main:
    increment: Major'
tagcommit v1.0.0
commit a
record

# branches.main.increment: Minor - 브랜치별 직접 increment.
newrepo cfg_branch_increment_minor main
writeconfig 'branches:
  main:
    increment: Minor'
tagcommit v1.0.0
commit a
record

# branches.main.mode: ContinuousDeployment - 연속 배포 모드(deployment mode 분기).
newrepo cfg_mode_cd_branch main
writeconfig 'branches:
  main:
    mode: ContinuousDeployment'
tagcommit v1.0.0
commit a; commit b
record

# assembly-file-versioning-scheme: MajorMinor - AssemblySemFileVer 분기.
newrepo cfg_assembly_file_scheme main
writeconfig 'assembly-file-versioning-scheme: MajorMinor'
tagcommit v1.2.3
commit a
record

# assembly-informational-format: 커스텀 - InformationalVersion 포맷 분기.
newrepo cfg_assembly_info_format main
writeconfig "assembly-informational-format: '{Major}.{Minor}.{Patch}-info'"
tagcommit v1.2.3
commit a
record

# branches.feature.label: 커스텀 pre-release label - PreReleaseLabel 분기.
newrepo cfg_branch_label_custom main
writeconfig 'workflow: GitHubFlow/v1
branches:
  feature:
    label: preview'
tagcommit v1.0.0
branch feature/x; commit f1
record

# version-in-branch-pattern: 커스텀 - release/2.3.0 에서 버전 추출(yaml escape 회피 [.]).
newrepo cfg_version_pattern_custom main
writeconfig 'version-in-branch-pattern: "^[vV]?(?<version>[0-9]+[.][0-9]+[.][0-9]+)"'
tagcommit v1.0.0
branch release/2.3.0; commit r1
record

# merge-message-formats: 커스텀 포맷 - 비표준 머지 메시지에서 SourceBranch 추출.
newrepo cfg_merge_format_custom main
writeconfig 'merge-message-formats:
  mycompany: "^Integrate (?<SourceBranch>[^ ]+) complete"'
tagcommit v1.0.0
branch release/2.0.0; commit r1; commit r2
checkout main
merge release/2.0.0 "Integrate release/2.0.0 complete"
record

# branches.main.track-merge-message: false - 머지 메시지 버전 추적 비활성.
newrepo cfg_track_merge_msg_false main
writeconfig 'branches:
  main:
    track-merge-message: false'
tagcommit v1.0.0
branch release/2.0.0; commit r1; commit r2
checkout main
merge release/2.0.0 "Merge branch '\''release/2.0.0'\''"
record

# branches.feature.pre-release-weight: 커스텀 - WeightedPreReleaseNumber 분기.
newrepo cfg_prerelease_weight main
writeconfig 'workflow: GitHubFlow/v1
branches:
  feature:
    pre-release-weight: 5000'
tagcommit v1.0.0
branch feature/x; commit f1
record

# assembly-file-versioning-format: 커스텀 - AssemblySemFileVer 포맷 분기.
newrepo cfg_assembly_file_format main
writeconfig "assembly-file-versioning-format: '{Major}.{Minor}.{Patch}.0'"
tagcommit v1.2.3
commit a
record

# commit-message-incrementing: Enabled (명시) - +semver 메시지 활성(기본).
newrepo cfg_msg_inc_enabled main
writeconfig 'commit-message-incrementing: Enabled'
tagcommit v1.0.0
commit "feat
+semver: minor"
record

# assembly-versioning-scheme: MajorMinorPatch (기본 명시) - AssemblySemVer 분기.
newrepo cfg_assembly_scheme_default main
writeconfig 'assembly-versioning-scheme: MajorMinorPatch'
tagcommit v1.2.3
commit a
record

# pull-request 브랜치 + label-number-pattern: PR 번호를 pre-release 번호로 추출.
newrepo cfg_pr_label_number main
writeconfig 'workflow: GitHubFlow/v1'
tagcommit v1.0.0
branch pull/123/merge
commit f1
record

# increment: Inherit (브랜치별) - feature 가 부모(main) 증분을 상속.
# main 을 Major 로 두고 feature 가 Inherit 이면 feature 도 Major 기반.
newrepo cfg_increment_inherit main
writeconfig 'workflow: GitHubFlow/v1
branches:
  main:
    increment: Major
  feature:
    increment: Inherit'
tagcommit v1.0.0
branch feature/x; commit f1
record

# branches.main.increment: None - 증분 안 함. 태그 후 커밋도 코어 동일(1.0.0).
newrepo cfg_branch_increment_none main
writeconfig 'branches:
  main:
    increment: None'
tagcommit v1.0.0
commit a
record

# assembly-file-versioning-scheme: None - AssemblySemFileVer 가 빈 문자열.
newrepo cfg_assembly_file_scheme_none main
writeconfig 'assembly-file-versioning-scheme: None'
tagcommit v1.2.3
commit a
record

# branches.feature.label-number-pattern: 커스텀 - 비표준 번호 추출 패턴.
newrepo cfg_label_number_custom main
writeconfig 'workflow: GitHubFlow/v1
branches:
  feature:
    label-number-pattern: "[/-](?<number>[0-9]+)"'
tagcommit v1.0.0
branch feature/issue-42; commit f1
record

# assembly-file-versioning-scheme: Major - AssemblySemFileVer = Major.0.0.0.
newrepo cfg_assembly_file_scheme_major main
writeconfig 'assembly-file-versioning-scheme: Major'
tagcommit v1.2.3
commit a
record

# branches.develop.track-merge-target: false - 머지 타겟 추적 비활성 분기.
newrepo cfg_track_merge_target_false main
writeconfig 'branches:
  develop:
    track-merge-target: false'
tagcommit v1.0.0
branch develop; commit d1; commit d2
record

# branches.feature.source-branches: 명시 - feature 의 부모 브랜치 정의.
newrepo cfg_source_branches main
writeconfig 'workflow: GitHubFlow/v1
branches:
  feature:
    source-branches:
      - main'
tagcommit v1.0.0
branch feature/x; commit f1
record

# is-source-branch-for + increment Inherit: custom 이 main(Major)을 source 로 상속해
# Major 증분(2.0.0). label 은 명시(cust)해 sanitize/fallback 변수를 배제.
newrepo cfg_is_source_branch_for main
writeconfig 'branches:
  main:
    increment: Major
    is-source-branch-for: [custom]
  custom:
    regex: "^custom/"
    increment: Inherit
    label: cust'
tagcommit v1.0.0
branch custom/x; commit b
record

# source-branches 로 increment Inherit 상속: custom 이 main(Major) 상속(2.0.0).
newrepo cfg_source_branches_inherit main
writeconfig 'branches:
  main:
    increment: Major
  custom:
    regex: "^custom/"
    increment: Inherit
    source-branches: [main]
    label: cust'
tagcommit v1.0.0
branch custom/x; commit b
record

# source-branches 로 label 상속: custom(label 미지정)이 main(label="", Major)에서
# label 과 increment 를 모두 상속해 2.0.0-1(label 없는 pre-release).
newrepo cfg_source_branches_label_inherit main
writeconfig 'branches:
  main:
    increment: Major
  custom:
    regex: "^custom/"
    increment: Inherit
    source-branches: [main]'
tagcommit v1.0.0
branch custom/x; commit b
record

# GitFlow unknown 브랜치(misc/foo): 명시 타입에 안 맞는 브랜치의 기본 동작.
newrepo cfg_gitflow_unknown main
tagcommit v1.0.0
branch misc/foo; commit u1
record

# GitHubFlow unknown 브랜치(misc/foo): 워크플로별 unknown 처리 차이.
newrepo cfg_githubflow_unknown main
writeconfig 'workflow: GitHubFlow/v1'
tagcommit v1.0.0
branch misc/foo; commit u1
record

# release 브랜치 + mode: ContinuousDeployment 직접 - 브랜치별 deployment mode 조합.
newrepo cfg_release_mode_cd main
writeconfig 'branches:
  release:
    mode: ContinuousDeployment'
tagcommit v1.0.0
branch release/2.0.0; commit r1; commit r2
record

# develop 브랜치 + mode: ManualDeployment 직접 - 비 mainline 수동 배포 조합.
newrepo cfg_develop_mode_manual main
writeconfig 'branches:
  develop:
    mode: ManualDeployment'
tagcommit v1.0.0
branch develop; commit d1; commit d2
record

# semantic-version-format: Loose + 부분 next-version("1"): Loose 는 "1" 을 1.0.0 으로
# 파싱(Strict 는 파싱 실패로 계산 에러). next-version + format 조합 검증.
newrepo cfg_nextversion_loose_partial main
writeconfig 'semantic-version-format: Loose
next-version: "1"'
commit a; commit b
record

# 정수 next-version(2) + Loose: 원본 setter 가 "2.0" 으로 보정 후 2.0.0 으로 파싱.
newrepo cfg_nextver_integer main
writeconfig 'semantic-version-format: Loose
next-version: 2'
commit a; commit b
record

# next-version + build metadata("1.0.0+build5"): build 부분이 있어도 정상 파싱.
newrepo cfg_nextver_build main
writeconfig 'next-version: "1.0.0+build5"'
tagcommit v1.0.0; commit b
record

# next-version pre-release("1.0.0-beta.3") on main: label 불일치로 무시되고 태그 기반.
newrepo cfg_nextver_prerelease main
writeconfig 'next-version: "1.0.0-beta.3"'
tagcommit v1.0.0; commit b
record

# tag-prefix 빈 문자열: "v1.0.0" 태그가 prefix 없이 파싱 실패해 fallback(0.0.x).
newrepo cfg_empty_tagprefix main
writeconfig 'tag-prefix: ""'
tagcommit v1.0.0; commit b
record

# detached HEAD: HEAD 커밋이 main 의 유일 tip 이면 원본처럼 main 으로 계산.
newrepo detached_head_main main
tagcommit v1.0.0; commit b
git -C "$CUR" -c advice.detachedHead=false checkout -q "$(git -C "$CUR" rev-parse HEAD)"
record

# detached HEAD: HEAD 가 feature/y 의 유일 tip 이면 feature/y 로 계산.
newrepo detached_head_feature main
tagcommit v1.0.0
branch feature/y; commit b
git -C "$CUR" -c advice.detachedHead=false checkout -q "$(git -C "$CUR" rev-parse HEAD)"
record

# detached HEAD + 여러 브랜치 동일 커밋: BranchName="(no branch)", label sanitize 검증
# ("(no branch)" 가 pre-release label 로 "-no-branch-" 로 정규화).
newrepo detached_no_branch main
tagcommit v1.0.0; commit b
git -C "$CUR" branch feature/x
git -C "$CUR" -c advice.detachedHead=false checkout -q "$(git -C "$CUR" rev-parse HEAD)"
record

# 브랜치명 언더스코어: feature/a_b 의 label 은 "a-b"(_ 가 - 로 sanitize, 원본 SanitizeName).
newrepo branch_underscore_label main
tagcommit v1.0.0
branch feature/a_b; commit f1
record

# 어노테이티드 태그(git tag -a): lightweight 와 동일하게 peel 되어 인식되어야 한다.
newrepo annotated_tag main
commit a
git -C "$CUR" tag -a v1.0.0 -m "release 1.0.0"
commit b
record

# +semver: breaking (major 별칭) - breaking 이 major 증분으로 인식.
newrepo semver_breaking main
tagcommit v1.0.0
commit "feat
+semver: breaking"
record

# +semver: none - 증분 억제.
newrepo semver_none main
tagcommit v1.0.0
commit "chore
+semver: none"
record

# 커스텀 major-version-bump-message: "BREAKING" 매칭 커밋이 major 증분(2.0.0).
newrepo cfg_custom_major_bump main
writeconfig 'major-version-bump-message: "BREAKING"'
tagcommit v1.0.0
commit "BREAKING change here"
record

# 커스텀 minor-version-bump-message: "^feat" 매칭 커밋이 minor 증분(1.1.0).
newrepo cfg_custom_minor_bump main
writeconfig 'minor-version-bump-message: "^feat"'
tagcommit v1.0.0
commit "feat: add thing"
record

# 커스텀 bump 패턴은 기본 +semver 를 대체: minor 를 "^feat" 로 바꾸면 "+semver: minor"
# 는 더 이상 매칭되지 않아 patch 만 증분(1.0.1).
newrepo cfg_custom_bump_overrides main
writeconfig 'minor-version-bump-message: "^feat"'
tagcommit v1.0.0
commit "+semver: minor"
record

# 커스텀 bump + 복수 커밋: major/minor 매칭이 혼재하면 최고(major) 증분(2.0.0).
newrepo cfg_custom_bump_multi main
writeconfig 'major-version-bump-message: "BREAKING"
minor-version-bump-message: "^feat"'
tagcommit v1.0.0
commit "feat: x"
commit "BREAKING: y"
commit "feat: z"
record

# 커스텀 bump + 복수 커밋: feat 여러 개여도 minor 는 한 번만 적용(1.1.0).
newrepo cfg_custom_bump_multi_minor main
writeconfig 'minor-version-bump-message: "^feat"'
tagcommit v1.0.0
commit "feat: x"
commit "chore: y"
commit "feat: z"
record

echo "압축: $OUT"
tar -C "$STAGE" -czf "$OUT" .
echo "완료. 시나리오 수: $(ls "$STAGE" | wc -l | tr -d ' ')"
