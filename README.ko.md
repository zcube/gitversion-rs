# gitversion (Rust 포트)

[English](README.md) · **한국어** · [日本語](README.ja.md) · [中文](README.zh.md)

[GitVersion](https://gitversion.net) (.NET) 을 Rust 로 포팅한 구현입니다. Git 히스토리로부터
의미론적 버전(SemVer)을 계산합니다.

> **프로젝트 목표: .NET 환경이 없는 곳에서도 작은 노력으로 GitVersion 을 실행한다.**
> .NET 런타임이나 전역 도구 설치 없이 동작하는 단일 네이티브 바이너리. 순수 Rust Git 접근
> (libgit2/C 의존성 없음)으로, 실제 GitVersion 바이너리와 차등(differential) 검증됩니다.

CLI·대화형 TUI·모든 내부 메시지가 [`rust-i18n`](https://github.com/longbridge/rust-i18n) 으로
**완전히 다국어화**(영어/한국어/일본어/중국어)되어 있습니다. 기본 언어는 영어이며,
`--lang ko|ja|zh` 또는 `LANG`/`LC_ALL` 환경변수로 변경합니다.

## 특징

- **순수 Rust git 접근**: [`gix`](https://github.com/GitoxideLabs/gitoxide) (gitoxide) — libgit2 등 C 의존성 없음
- **CLI**: [`clap`](https://docs.rs/clap)
- **로깅**: [`env_logger`](https://docs.rs/env_logger) (`RUST_LOG` 또는 `--verbosity`/`--diag`)
- **i18n**: [`rust-i18n`](https://github.com/longbridge/rust-i18n), 기본 영어, `--lang`/`LANG`, `locales/app.yml`
- **TUI**: [`ratatui`](https://ratatui.rs) (`--tui`) — 5개 탭(변수/설정/커밋/브랜치/액션).
  변수 검색·복사, **설정 탭에서 전역 설정 편집**(Enter)하면 그 아래 effective 결과가 즉시
  갱신되고 **GitVersion.yml 에 최소 diff 로 저장되어 유지**됨, first-parent 커밋과 버전 소스
  표시, 브랜치 선택 재계산, 액션(태그·브랜치 생성, next-version 설정, **Conventional Commits
  토글(저장됨)**, **exec 훅 편집·실행**, 설정 저장, 캐시 삭제, 동적 clone, 재계산).
  version 훅은 즉시 재계산에 반영. 패닉이 나도 터미널을 복구하고 우아하게 종료(catch_unwind)
- **워크플로**: GitFlow / GitHubFlow / TrunkBased(Mainline)
- **버전 전략**: ConfiguredNextVersion, TaggedCommit, MergeMessage, VersionInBranchName,
  TrackReleaseBranches, Fallback, Mainline
- **증분 규약**: GitVersion `+semver:` 방식과 **Conventional Commits**(`feat`→minor,
  `fix`/`perf`→patch, `feat!`·`BREAKING CHANGE:`→major)를 선택
  (`commit-message-convention: ConventionalCommits`). semantic-release 검토에서 차용
- **배포 모드**: ManualDeployment / ContinuousDelivery / ContinuousDeployment
- **출력**: JSON, dot-env, build-server, 단일 변수(`-v`), 포맷 문자열(`--format`)
- **로그 파일**: `--log`/`-l <FILE>`(원본 `/l`)로 타임스탬프 로그를 파일에 append. stdout 은 버전
  결과 전용으로 깨끗하게 유지
- **빌드에이전트 통합**: TeamCity, Azure Pipelines, GitHub Actions, GitLab CI, Jenkins,
  AppVeyor, TravisCI, Drone, CodeBuild, ContinuaCI, EnvRun, MyGet, BitBucket, BuildKite,
  SpaceAutomation — 환경변수로 감지해 각 CI 형식으로 출력(`--output build-server`)
- **파일 출력**: AssemblyInfo 갱신/생성(`--updateassemblyinfo [파일] [--ensureassemblyinfo]`),
  프로젝트 파일 갱신(`--updateprojectfiles`, 정규식이 아닌 실제 XML 파싱), Wix 버전 파일(`--updatewixversionfile`)
- **패키지 매니페스트**: `--updatepackagefiles` 로 `package.json`(Node.js), `Cargo.toml`(Rust),
  `pyproject.toml`(Python, PEP 621/Poetry)의 version 을 포맷 보존 파서(serde_json/toml_edit)로 갱신
- **외부 명령 훅(exec)**: semantic-release 의 exec 플러그인처럼 라이프사이클 훅
  (`verify`/`prepare`/`publish`/`success`/`fail`)에서 쉘 명령 실행. 버전 변수를 `GitVersion_*`
  환경변수와 `{Variable}`/`{env:VAR}` 템플릿으로 노출. `version` 훅은 명령의 표준출력으로 버전을
  수정(next-version 적용 후 재계산). `--exec`/`--exec-version`/`--dry-run` 지원
- **결과 캐싱**: 계산 결과를 `<.git>/gitversion_cache/<키>.json` 에 저장해 재사용. 키는
  refs·HEAD·설정파일·overrideconfig 의 SHA1 해시라 저장소 상태가 바뀌면 자동 무효화. `--nocache` 로 비활성화
- **동적 원격 저장소**: `--url <repo> --branch <b>` 로 원격을 clone 해 계산(`-u`/`-p` 인증,
  `-c` 커밋, `--dynamicRepoLocation`). gix 순수 Rust clone 으로 https/file 및 SSH 지원
  - **자격증명 helper / OS 키링**: https 인증 시 git 의 credential helper 프로토콜을 사용한다.
    `-u`/`-p` 미지정 시 git 설정(`credential.helper`)을 호출하므로 macOS Keychain·GCM·libsecret
    등에 저장된 자격증명을 자동 사용한다

## 빌드

```bash
cargo build --release
```

## 사용

```bash
# 현재 디렉터리 저장소의 전체 변수를 JSON 으로 출력
gitversion

# 단일 변수 / 포맷 문자열
gitversion -v FullSemVer
gitversion --format "v{Major}.{Minor}.{Patch} ({EscapedBranchName})"

# 출력 형식
gitversion --output json
gitversion --output build-server

# 설정/오버라이드
gitversion --overrideconfig next-version=2.0.0
gitversion --showconfig

# 외부 명령 훅(exec)
gitversion --exec 'npm version {SemVer} --no-git-tag-version'
gitversion --exec-version './scripts/decide-version.sh'

# 대화형 TUI
gitversion --tui

# 언어(기본 영어)
gitversion --lang ko

# 특정 브랜치 기준 계산
gitversion -b release/2.0.0
```

## 설정 파일

작업 디렉터리(및 저장소 루트)에서 `GitVersion.yml`, `GitVersion.yaml`, `.GitVersion.yml`,
`.GitVersion.yaml` 을 탐색합니다. 키는 원본 GitVersion 과 동일한 kebab-case 입니다.

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
| `src/i18n.rs` + `locales/` | rust-i18n 로케일 처리 | (신규) |

> 참고: `refs/gitversion` 는 포팅 기준이 된 .NET 원본 소스이며 `.gitignore` 로 추적에서 제외됩니다.

## 테스트

실제 GitVersion 6.x 바이너리를 golden 기준으로 삼는 **차등(differential) 테스트**를 사용합니다.

```bash
cargo test
GITVERSION_BIN=/opt/homebrew/bin/gitversion ./tests/build_fixtures.sh
```

- `tests/build_fixtures.sh`: 시나리오별 git 저장소를 만들고 실제 GitVersion 을 돌려 golden
  기대값(`expected.json`)을 기록한 뒤 `testdata/fixtures.tar.gz` 로 압축.
- `tests/fixtures.rs`: 압축을 임시 디렉터리로 풀어 우리 엔진 출력을 golden 값과 필드 단위로
  비교. 테스트 시점에는 git/gitversion 이 불필요(재현 가능).

## 알려진 단순화 / 미구현

- `track-merge-target`: 원본의 `MainlineVersionStrategy` 와 `GetTaggedSemanticVersion()` 에서만
  소비되는 플래그입니다. 본 포트는 이미 HEAD 에서 도달 가능한 모든 태그를 후보로 보므로 도달
  가능한 merge-target 태그는 포괄되며, 도달 불가한 경우(주로 Mainline)는 미반영입니다.
- `/nofetch /nonormalize /allowshallow` 는 인식하지만 이 포트의 구조상 무효과인 정직한 no-op
  입니다(원격 clone 은 fetch/normalize 를 직접 수행).
- `GitVersionInformation` 소스 파일 생성은 원본에서도 CLI 가 아닌 MSBuild 태스크가 담당하므로
  본 CLI 포트의 범위 밖입니다.

검증은 실제 GitVersion 6.7.0 바이너리와의 차등 테스트(시나리오 × 출력 필드, 빌드에이전트 5종,
파일 출력)로 보장됩니다.
