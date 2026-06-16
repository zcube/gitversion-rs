# AGENTS.md

이 저장소에서 작업하는 에이전트/기여자를 위한 가이드입니다. 사용자용 개요는
[README](README.md)(다국어: [ko](README.ko.md) · [ja](README.ja.md) · [zh](README.zh.md))를 참고하세요.

## 프로젝트 개요

GitVersion(.NET)을 Rust 로 포팅한 단일 네이티브 바이너리. Git 히스토리로부터 SemVer 를
계산한다. 순수 Rust(gix), 실제 GitVersion 6.x 바이너리와 차등 테스트로 검증.

## 개발 워크플로

```bash
cargo build                  # 빌드
cargo test                   # 유닛 + fixture 통합 테스트
cargo fmt --all              # 포맷
cargo clippy --all-targets -- -D warnings   # 린트(경고 0 유지)
```

- **lefthook**(설치: `lefthook install`)
  - pre-commit: 스테이징된 `*.rs` 에 `cargo fmt`(자동 정렬·재스테이징) + `clippy -D warnings`
  - pre-push: `cargo test`
- **CI**(`.github/workflows/ci.yml`): fmt --check, clippy -D warnings, 3개 OS 빌드·테스트,
  MSRV(현재 1.88, 트랜지티브 의존성 floor) 빌드. main 푸시·PR 에서 동작.

## 커밋 규약

- **Conventional Commits** 타입 접두사 필수: `feat|fix|ci|chore|test|docs|refactor|perf|style|build|revert`.
- 커밋 메시지는 **한국어**로 작성.
- **Co-Authored-By 등 AI co-author 트레일러 금지**, 화살표(→)·이모지 등 특수문자 금지
  (commit-msg 훅이 거부함).
- 테스트용 임시 저장소 커밋은 `git commit --no-verify` 사용 가능.

## i18n (다국어)

- 크레이트: [`rust-i18n`](https://github.com/longbridge/rust-i18n). 기본 언어 **영어**,
  소스 키도 영문. 지원: en/ko/ja/zh.
- 번역은 `locales/app.yml`(`_version: 2`)에 `key: { en, ko, ja, zh }` 또는 블록 형태로 둔다.
  플레이스홀더는 `%{name}`.
- 코드에서는 `rust_i18n::t!("key", name = value)`. **t! 는 `i18n!` 매크로를 호출한 lib
  크레이트(`src/lib.rs`) 안에서만 동작**하므로 진입 로직은 `src/app.rs` 에 둔다(`main.rs` 는 shim).
- 런타임 변수 키는 `t!(*k)`(이중참조 역참조)로 넘긴다. CLI 헬프는 `cli::localized_command()`
  가 파싱 전에 `cli.about`/`cli.help.<arg_id>` 키로 주입한다.
- **새 사용자 문자열을 추가하면 반드시 4개 언어 값을 모두 `locales/app.yml` 에 추가**한다.
  키 누락 시 rust-i18n 은 키 문자열을 그대로 출력한다.

## 버전 관리와 릴리스

이 프로젝트는 **자기 자신(gitversion)으로 버전을 계산한다(dogfooding)**.

- **버전의 단일 기준은 git 태그(`v*`)** 와 `GitVersion.yml` 설정이다.
- `Cargo.toml` 의 `version` 은 **placeholder** 다. 손으로 올리지 않는다. 릴리스 빌드 시
  gitversion 이 산출한 값으로 **자동으로 덮어써서** 빌드하므로, 배포 바이너리의 `--version`
  은 실제 릴리스 버전을 보고한다.
- 태그가 없는 개발 빌드는 `GitVersion.yml` 의 `next-version`(현재 0.1.0)을 기준으로
  `0.1.0-<distance>` 같은 pre-release 를 산출한다.

### 릴리스 절차

1. `main` 이 그린인지 확인한다(CI 통과).
2. 다음 릴리스 버전을 정한다. 원하면 gitversion 으로 후보를 확인:
   ```bash
   cargo run -q -- -v SemVer        # 현재(태그 전) 버전
   ```
3. 릴리스 커밋에서 **주석 태그**를 만들고 푸시한다(태그 = 버전의 출처):
   ```bash
   git tag -a v0.1.0 -m "release: v0.1.0"
   git push origin v0.1.0
   ```
4. 태그 푸시가 `.github/workflows/release.yml` 을 트리거한다:
   - **version 잡**: 태그된 커밋에서 gitversion 을 빌드·실행해 `SemVer` 를 산출(태그가
     있으므로 `v0.1.0 -> 0.1.0`).
   - **build 잡**(6개 타깃: Linux x86_64/aarch64/musl, macOS x86_64/aarch64, Windows x86_64):
     산출 버전을 `Cargo.toml` 에 주입한 뒤 빌드하고, 아카이브(README 4종 + LICENSE 동봉)를
     GitHub Release 에 업로드한다.
5. 결과: GitHub Release 에 플랫폼별 바이너리가 첨부되고, 각 바이너리 `gitversion --version`
   은 태그 버전을 보고한다.
6. **Homebrew tap 자동 갱신**: 빌드가 끝나면 `homebrew` 잡이 자산의 SHA256 을 계산해
   `zcube/homebrew-tap` 의 `Formula/gitversion.rb` 를 새 버전으로 덮어쓰고 커밋·푸시한다.
   - 필요 시크릿: **`HOMEBREW_TAP_TOKEN`** — `zcube/homebrew-tap` 에 contents:write 권한이
     있는 PAT(또는 fine-grained 토큰). gitversion-rs 저장소 Secrets 에 등록한다.
   - 시크릿이 없으면 이 잡은 조용히 건너뛴다(릴리스 자체에는 영향 없음).
   - 사전 릴리스(버전에 `-` 포함, 예: `0.1.0-rc.1`)에는 tap 을 갱신하지 않는다.
   - 설치: `brew install zcube/tap/gitversion`.

> 수동 재빌드가 필요하면 Actions 의 `Release` 워크플로를 `workflow_dispatch` 로 실행하고
> 태그명을 입력한다.

### 버전을 올리려면

- 다음 사이클의 기준을 바꾸려면 `GitVersion.yml` 의 `next-version` 을 갱신하거나, 단순히
  더 높은 `v*` 태그를 만든다(태그가 항상 우선).

## 코드 구조

| 모듈 | 역할 |
|---|---|
| `src/git` | gix 기반 저장소 접근 |
| `src/config` | 설정 모델/기본값/로더/effective |
| `src/version` | SemanticVersion 및 계산 엔진 |
| `src/output` | 출력 변수/포맷터/파일 출력 |
| `src/cli` | clap 인자 + `localized_command()` |
| `src/app.rs` | 진입 로직(t! 사용 위해 lib 안) |
| `src/tui` | ratatui TUI |
| `src/i18n.rs` + `locales/` | rust-i18n 로케일 처리 |

> `refs/gitversion` 은 포팅 기준이 된 .NET 원본 소스이며 `.gitignore` 로 추적 제외된다.
