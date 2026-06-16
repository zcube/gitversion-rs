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
    git -C "$CUR" merge --no-ff -q "$1" -m "$2"
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

echo "압축: $OUT"
tar -C "$STAGE" -czf "$OUT" .
echo "완료. 시나리오 수: $(ls "$STAGE" | wc -l | tr -d ' ')"
