# gitversion (Rust 포트)

[GitVersion](https://gitversion.net) (.NET) 을 Rust 로 포팅한 구현입니다. Git 히스토리로부터
의미론적 버전(SemVer)을 계산합니다.

## 특징

- **순수 Rust git 접근**: [`gix`](https://github.com/GitoxideLabs/gitoxide) (gitoxide) 사용 — libgit2 등 C 의존성 없음
- **CLI**: [`clap`](https://docs.rs/clap)
- **로깅**: [`env_logger`](https://docs.rs/env_logger) (`RUST_LOG` 또는 `--verbosity`/`--diag`)
- **TUI**: [`ratatui`](https://ratatui.rs) (`--tui`)
- **워크플로**: GitFlow / GitHubFlow / TrunkBased(Mainline)
- **버전 전략**: ConfiguredNextVersion, TaggedCommit, MergeMessage, VersionInBranchName,
  TrackReleaseBranches, Fallback, (Mainline 단순화)
- **배포 모드**: ManualDeployment / ContinuousDelivery / ContinuousDeployment
- **출력**: JSON, dot-env, build-server env, 단일 변수(`-v`), 포맷 문자열(`--format`)

## 빌드

```bash
cargo build --release
```

## 사용

```bash
# 현재 디렉터리 저장소의 전체 변수를 JSON 으로 출력
gitversion

# 단일 변수
gitversion -v FullSemVer
gitversion -v SemVer

# 포맷 문자열
gitversion --format "v{Major}.{Minor}.{Patch} ({EscapedBranchName})"

# 출력 형식
gitversion --output json
gitversion --output dot-env
gitversion --output build-server

# 설정/오버라이드
gitversion --config GitVersion.yml
gitversion --overrideconfig next-version=2.0.0
gitversion --showconfig

# 대화형 TUI
gitversion --tui

# 특정 브랜치 기준 계산
gitversion -b release/2.0.0
```

## 설정 파일

작업 디렉터리(및 저장소 루트)에서 `GitVersion.yml`, `GitVersion.yaml`,
`.GitVersion.yml`, `.GitVersion.yaml` 을 탐색합니다. 키는 원본 GitVersion 과 동일한
kebab-case 입니다.

```yaml
workflow: GitFlow/v1
next-version: 1.0.0
tag-prefix: "[vV]?"
branches:
  develop:
    increment: Minor
    label: alpha
```

## 프로젝트 구조

| 모듈 | 역할 | 원본 대응 |
|---|---|---|
| `src/git` | gix 기반 저장소 접근 | `GitVersion.LibGit2Sharp` |
| `src/config` | 설정 모델 / 워크플로 기본값 / 로더 / effective | `GitVersion.Configuration` |
| `src/version` | SemanticVersion 및 계산 엔진 | `GitVersion.Core` |
| `src/output` | 출력 변수 / 포맷터 | `GitVersion.Output` |
| `src/cli` | clap 인자 | `GitVersion.App` |
| `src/tui` | ratatui UI | (신규) |

> 참고: `refs/gitversion` 는 포팅 기준이 된 .NET 원본 소스이며 `.gitignore` 로 추적에서 제외됩니다.

## 테스트

실제 GitVersion 6.x 바이너리를 golden 기준으로 삼는 **차등(differential) 테스트**를 사용합니다.

```bash
# 전체 테스트 (유닛 + fixture 통합)
cargo test

# fixture 재생성 (실제 gitversion 바이너리 필요)
GITVERSION_BIN=/opt/homebrew/bin/gitversion ./tests/build_fixtures.sh
```

- `tests/build_fixtures.sh`: 시나리오별 git 저장소를 만들고 실제 GitVersion 을 돌려
  golden 기대값(`expected.json`)을 기록한 뒤 `testdata/fixtures.tar.gz` 로 압축.
- `tests/fixtures.rs`: 압축을 임시 디렉터리로 풀어 우리 엔진 출력을 golden 값과
  필드 단위로 비교. 테스트 시점에는 git/gitversion 이 불필요(재현 가능).
- 현재 **22개 시나리오 × 19개 출력 필드**가 실제 GitVersion 6.7.0 과 일치
  (main/develop/release/feature/hotfix/support, +semver 메시지, GitHubFlow,
  next-version, custom tag-prefix, pre-release 태그, 다중 태그, ignore.sha,
  custom commit-date-format 등).

## 알려진 단순화

- `Inherit` 증분의 source-branch 해석은 git 조상 추적 대신 설정상 첫 번째 source 를
  따릅니다(공통 GitFlow 시나리오에서는 동일한 결과).
- Mainline 전략은 TaggedCommit 기반의 단순화된 형태입니다(워크플로 `TrunkBased` 미지원).
- `merge-message-formats`, `semantic-version-format`(Strict/Loose 구분),
  `assembly-versioning-format`/`assembly-informational-format` 커스텀 포맷은 미반영입니다.
- AssemblyInfo/프로젝트 파일 쓰기, 동적(원격) 저장소 clone, 캐싱, 빌드에이전트별 통합은 미구현입니다.
