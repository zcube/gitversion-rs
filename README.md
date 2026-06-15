# gitversion (Rust 포트)

[GitVersion](https://gitversion.net) (.NET) 을 Rust 로 포팅한 구현입니다. Git 히스토리로부터
의미론적 버전(SemVer)을 계산합니다.

## 특징

- **순수 Rust git 접근**: [`gix`](https://github.com/GitoxideLabs/gitoxide) (gitoxide) 사용 — libgit2 등 C 의존성 없음
- **CLI**: [`clap`](https://docs.rs/clap)
- **로깅**: [`env_logger`](https://docs.rs/env_logger) (`RUST_LOG` 또는 `--verbosity`/`--diag`)
- **TUI**: [`ratatui`](https://ratatui.rs) (`--tui`) — 5개 탭(변수/설정/커밋/브랜치/액션).
  변수 검색·복사, effective 설정 보기, first-parent 커밋과 버전 소스 표시, 브랜치 선택
  재계산, 액션(태그·브랜치 생성, next-version 설정, 캐시 삭제, 동적 clone, 재계산).
  패닉이 나도 터미널을 복구하고 우아하게 종료(catch_unwind 방어)
- **워크플로**: GitFlow / GitHubFlow / TrunkBased(Mainline)
- **버전 전략**: ConfiguredNextVersion, TaggedCommit, MergeMessage, VersionInBranchName,
  TrackReleaseBranches, Fallback, (Mainline 단순화)
- **배포 모드**: ManualDeployment / ContinuousDelivery / ContinuousDeployment
- **출력**: JSON, dot-env, build-server, 단일 변수(`-v`), 포맷 문자열(`--format`)
- **빌드에이전트 통합**: TeamCity, Azure Pipelines, GitHub Actions, GitLab CI, Jenkins,
  AppVeyor, TravisCI, Drone, CodeBuild, ContinuaCI, EnvRun, MyGet, BitBucket, BuildKite,
  SpaceAutomation — 환경변수로 감지해 각 CI 형식으로 출력(`--output build-server`)
- **파일 출력**: AssemblyInfo 갱신/생성(`--updateassemblyinfo [파일] [--ensureassemblyinfo]`),
  프로젝트 파일 갱신(`--updateprojectfiles`, 정규식이 아닌 실제 XML 파싱으로 안전하게
  갱신·삽입), Wix 버전 파일(`--updatewixversionfile`)
- **패키지 매니페스트**: `--updatepackagefiles` 로 `package.json`(Node.js),
  `Cargo.toml`(Rust), `pyproject.toml`(Python, PEP 621/Poetry)의 version 을 각 형식의
  포맷 보존 파서(serde_json/toml_edit)로 갱신
- **결과 캐싱**: 계산 결과를 `<.git>/gitversion_cache/<키>.json` 에 저장해 재사용. 키는
  refs·HEAD·설정파일·overrideconfig 의 SHA1 해시라 저장소 상태가 바뀌면 자동 무효화.
  `--nocache` 로 비활성화
- **동적 원격 저장소**: `--url <repo> --branch <b>` 로 원격을 clone 해 계산(`-u`/`-p` 인증,
  `-c` 커밋 지정, `--dynamicRepoLocation` 위치 지정). gix 순수 Rust clone 으로 https/file 및
  SSH(`ssh://`·scp-like `git@host:path`, 시스템 ssh 사용) 지원
  - **자격증명 helper / OS 키링**: https 인증 시 git 의 credential helper 프로토콜을 그대로
    사용한다. `-u`/`-p` 미지정 시 git 설정(`credential.helper`)을 호출하므로 macOS
    Keychain(`osxkeychain`)·GCM·libsecret 등에 저장된 자격증명을 자동 사용한다(get/erase
    전체 프로토콜). helper 에 없으면 TTY 프롬프트 가능

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
- 현재 **27개 시나리오 × 22개 출력 필드**가 실제 GitVersion 6.7.0 과 일치
  (main/develop/release/feature/hotfix/support, +semver 메시지, GitHubFlow,
  next-version, custom tag-prefix, pre-release 태그, 다중 태그, ignore.sha,
  custom commit-date-format, semantic-version-format Strict/Loose,
  assembly 커스텀 포맷, feature off main 의 increment 상속, tag-pre-release-weight 등).

## 설정 반영 현황

다음 설정은 실제 GitVersion 과 동일하게 **반영**됩니다:
`workflow`, `tag-prefix`, `version-in-branch-pattern`, `next-version`, `increment`,
`mode`, `label`, `regex`, `strategies`, `commit-message-incrementing`,
`major/minor/patch/no-bump-version-bump-message`, `source-branches`,
`tracks-release-branches`, `is-release-branch`, `pre-release-weight`,
`tag-pre-release-weight`, `prevent-increment.*`, `track-merge-message`,
`ignore`(sha·commits-before), `commit-date-format`, `semantic-version-format`,
`assembly-versioning-scheme`/`-format`, `assembly-file-versioning-scheme`/`-format`,
`assembly-informational-format`, `merge-message-formats`, `is-source-branch-for`.

- `Inherit` 증분은 git 조상을 추적해 실제로 분기한 source 브랜치의 증분을 상속합니다.
- merge 메시지는 8종 내장 포맷(Default/SmartGit/BitBucket/GitHub/AzureDevOps/RemoteTracking 등)과
  사용자 정의 포맷을 인식하며, 병합된 브랜치가 release 브랜치일 때만 버전을 사용합니다.
- 라벨은 정규식 named capture 외에 `{env:VAR}` 환경변수와 `{X ?? "기본값"}` 폴백을 지원합니다.
- `is-source-branch-for` 는 대상 브랜치의 `source-branches` 로 역매핑됩니다.
- 설정 검증: 각 브랜치에 `regex` 필수, `source-branches` 는 설정된 브랜치만 참조 가능
  (원본과 동일한 메시지로 에러).

## 알려진 단순화 / 미구현

- Mainline 전략은 base(최고 태그 또는 0.0.0)부터 각 커밋의 증분을 누적하는 방식으로
  구현되어 선형 히스토리에서 실제 GitVersion 과 일치합니다(`strategies: [Mainline]` +
  `mode: ContinuousDeployment`). 복잡한 merge 기반 브랜치 순회(18종 incrementer)는 단순화되어
  있으며, 워크플로 문자열 `TrunkBased` 는 실제 6.7 도 미지원입니다.
- `update-build-number`: `--output build-server` 시 빌드에이전트의 build number 설정
  출력을 제어합니다(false 면 생략). 계산되는 버전 변수에는 영향이 없습니다(원본과 동일).
- `track-merge-target`: 원본에서 `MainlineVersionStrategy` 와
  `GetTaggedSemanticVersion()`(태그 후보에 *merge target* 태그를 추가) 에서만 소비되는
  플래그입니다. 본 포트는 이미 HEAD 에서 도달 가능한 모든 태그를 후보로 보므로 도달
  가능한 merge-target 태그는 포괄되며, 도달 불가한 경우(주로 Mainline)는 미반영입니다.
- 로그 파일 출력(`/l`)은 미구현입니다. `/nofetch /nonormalize /allowshallow` 는 인식하지만
  이 포트의 구조상 무효과인 정직한 no-op 입니다(원격 clone 은 fetch/normalize 를 직접 수행).
- `GitVersionInformation` 소스 파일 생성은 원본에서도 CLI 가 아닌 MSBuild 태스크가
  담당하므로 본 CLI 포트의 범위 밖입니다.

## 원본 대비 커버리지 요약

| 영역 | 상태 |
|---|---|
| CLI 옵션 27종 | 핵심 18종 구현 + no-op 4종 + 원격/로그 5종 미구현 |
| 설정(config schema) | 전 필드 파싱, 대부분 동작 반영(위 "설정 반영 현황") |
| 버전 전략 | ConfiguredNextVersion·TaggedCommit·MergeMessage·VersionInBranchName·TrackReleaseBranches·Fallback·Mainline 구현 |
| 배포 모드 | Manual / ContinuousDelivery / ContinuousDeployment 구현 |
| 출력 | JSON·dot-env·build-server(에이전트 15종)·showvariable·format·파일(AssemblyInfo/proj/Wix) |
| 워크플로 | GitFlow·GitHubFlow 구현, TrunkBased 는 strategies+mode 로 대체 |

검증은 실제 GitVersion 6.7.0 바이너리와의 차등 테스트(31 시나리오 × 22 필드, 빌드에이전트
5종, 파일 출력)로 보장됩니다.
