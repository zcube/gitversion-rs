# gitversion (Rust port)

[English](README.md) · [한국어](README.ko.md) · **日本語** · [中文](README.zh.md)

[GitVersion](https://gitversion.net)（.NET）の Rust 移植版です。Git の履歴からセマンティック
バージョン（SemVer）を計算します。

> **プロジェクトの目標: .NET のない環境で、最小限の手間で GitVersion を実行すること。**
> 単一の自己完結型ネイティブバイナリ — .NET ランタイム不要、グローバルツールのインストール不要。
> Pure-Rust による Git アクセス（libgit2/C 依存なし）で、本物の GitVersion バイナリと
> 差分検証されています。

CLI、対話型 TUI、およびすべての内部メッセージは [`rust-i18n`](https://github.com/longbridge/rust-i18n)
によって**完全に国際化**されています（英語 / 韓国語 / 日本語 / 中国語）。
デフォルトは英語で、`--lang ko|ja|zh` または `LANG`/`LC_ALL` 環境変数で上書きできます。

## Features

- **Pure-Rust による Git アクセス**: [`gix`](https://github.com/GitoxideLabs/gitoxide)（gitoxide）— libgit2/C 依存なし
- **CLI**: [`clap`](https://docs.rs/clap)
- **ロギング**: [`env_logger`](https://docs.rs/env_logger)（`RUST_LOG`、または `--verbosity`/`--diag`）
- **i18n**: [`rust-i18n`](https://github.com/longbridge/rust-i18n)、デフォルト英語、`--lang`/`LANG`、ロケールは `locales/app.yml`
- **TUI**: [`ratatui`](https://ratatui.rs)（`--tui`）— 5 つのタブ（Variables/Config/Commits/Branches/Actions）。
  変数の検索とコピー、**Config タブでグローバル設定を編集**（Enter）でき、実効結果が即座に
  更新され、**最小限の差分として GitVersion.yml に保存**されます。バージョンソースが
  マークされた first-parent コミット、ブランチごとの再計算、各種アクション（タグ/ブランチの作成、
  next-version の設定、**Conventional Commits の切り替え（永続化）**、**exec フックの編集/実行**、
  設定の保存、キャッシュのクリア、動的クローン、再計算）。バージョンフックは即座に反映されます。
  パニックは捕捉され（`catch_unwind`）、ターミナルは復元され、正常に終了します
- **ワークフロー**: GitFlow / GitHubFlow / TrunkBased (Mainline)
- **バージョン戦略**: ConfiguredNextVersion, TaggedCommit, MergeMessage, VersionInBranchName,
  TrackReleaseBranches, Fallback, Mainline
- **インクリメント規約**: GitVersion の `+semver:` と **Conventional Commits**（`feat`→minor,
  `fix`/`perf`→patch, `feat!`/`BREAKING CHANGE:`→major）。
  `commit-message-convention: ConventionalCommits` で選択可能（semantic-release のレビューから借用）
- **デプロイモード**: ManualDeployment / ContinuousDelivery / ContinuousDeployment
- **出力**: JSON, dot-env, build-server, 単一変数（`-v`）, フォーマット文字列（`--format`）
- **ログファイル**: `--log`/`-l <FILE>`（原本 `/l`）でタイムスタンプ付きログをファイルに追記。
  stdout はバージョン結果専用にクリーンなまま
- **ビルドエージェント連携**: TeamCity, Azure Pipelines, GitHub Actions, GitLab CI, Jenkins,
  AppVeyor, TravisCI, Drone, CodeBuild, ContinuaCI, EnvRun, MyGet, BitBucket, BuildKite,
  SpaceAutomation — 環境から自動検出され、各 CI のフォーマットで出力されます（`--output build-server`）
- **ファイル出力**: AssemblyInfo の更新/作成（`--updateassemblyinfo [file] [--ensureassemblyinfo]`）、
  プロジェクトファイルの更新（`--updateprojectfiles`、正規表現ではなく実際の XML パース）、
  Wix バージョンファイル（`--updatewixversionfile`）
- **パッケージマニフェスト**: `--updatepackagefiles` は `package.json`（Node.js）、
  `Cargo.toml`（Rust）、`pyproject.toml`（Python, PEP 621/Poetry）のバージョンを、
  フォーマットを保持するパーサ（serde_json/toml_edit）で更新します
- **外部コマンドフック (exec)**: semantic-release の exec プラグインのように、ライフサイクルフック
  （`verify`/`prepare`/`publish`/`success`/`fail`）でシェルコマンドを実行します。バージョン変数は
  `GitVersion_*` 環境変数および `{Variable}`/`{env:VAR}` テンプレートとして公開されます。`version`
  フックはコマンドの stdout からバージョンを変更します（next-version を適用してから再計算）。
  `--exec`/`--exec-version`/`--dry-run` をサポートします
- **結果キャッシュ**: 結果は `<.git>/gitversion_cache/<key>.json` に保存されます。キーは
  refs·HEAD·設定ファイル·overrideconfig の SHA1 なので、リポジトリの状態が変わると自動的に
  無効化されます。`--nocache` で無効化できます
- **動的リモートリポジトリ**: `--url <repo> --branch <b>` でクローンして計算します（`-u`/`-p` 認証、
  `-c` コミット、`--dynamicRepoLocation`）。Pure-Rust の gix クローンで https/file および SSH
  （`ssh://`、scp 形式の `git@host:path`、システムの ssh を使用）に対応
  - **クレデンシャルヘルパー / OS キーリング**: https 認証では git のクレデンシャルヘルパープロトコルを
    使用します。`-u`/`-p` がない場合は設定された `credential.helper` を呼び出すため、macOS
    Keychain（`osxkeychain`）、GCM、libsecret などに保存された資格情報が自動的に使用されます
    （get/erase プロトコルに完全対応）

## Build

```bash
cargo build --release
```

## Usage

```bash
# Print all variables of the current repo as JSON
gitversion

# Single variable
gitversion -v FullSemVer

# Format string
gitversion --format "v{Major}.{Minor}.{Patch} ({EscapedBranchName})"

# Output formats
gitversion --output json
gitversion --output dot-env
gitversion --output build-server

# Config / overrides
gitversion --config GitVersion.yml
gitversion --overrideconfig next-version=2.0.0
gitversion --showconfig

# External command hooks (exec) — version variables exposed as env/templates
gitversion --exec 'npm version {SemVer} --no-git-tag-version'
gitversion --exec-version './scripts/decide-version.sh'
gitversion --exec 'make release' --dry-run

# Interactive TUI
gitversion --tui

# Language (default English)
gitversion --lang ko
gitversion --lang ja
gitversion --lang zh

# Compute for a specific branch
gitversion -b release/2.0.0
```

## Configuration file

作業ディレクトリ（およびリポジトリのルート）で `GitVersion.yml`、`GitVersion.yaml`、
`.GitVersion.yml`、`.GitVersion.yaml` を検索します。キーは上流の GitVersion と同じ
kebab-case を使用します。

```yaml
workflow: GitFlow/v1
next-version: 1.0.0
tag-prefix: "[vV]?"
branches:
  develop:
    increment: Minor
    label: alpha
```

## Project structure

| Module | 役割 | 上流の対応 |
|---|---|---|
| `src/git` | gix ベースのリポジトリアクセス | `GitVersion.LibGit2Sharp` |
| `src/config` | 設定モデル / ワークフローのデフォルト / ローダー / 実効設定 | `GitVersion.Configuration` |
| `src/version` | SemanticVersion と計算エンジン | `GitVersion.Core` |
| `src/output` | 出力変数 / フォーマッタ | `GitVersion.Output` |
| `src/cli` | clap の引数 | `GitVersion.App` |
| `src/tui` | ratatui UI | (new) |
| `src/i18n.rs` + `locales/` | rust-i18n ロケール処理 | (new) |

> 注: `refs/gitversion` はこの移植版のベースとなった .NET ソースです。`.gitignore` により
> トラッキングから除外されています。

## Testing

本物の GitVersion 6.x バイナリをゴールデンリファレンスとする**差分テスト**を使用します。

```bash
# Full test suite (unit + fixture integration)
cargo test

# Regenerate fixtures (requires the real gitversion binary)
GITVERSION_BIN=/opt/homebrew/bin/gitversion ./tests/build_fixtures.sh
```

- `tests/build_fixtures.sh`: シナリオごとの git リポジトリをビルドし、本物の GitVersion を実行して
  ゴールデンの `expected.json` を記録し、それらを `testdata/fixtures.tar.gz` にパックします。
- `tests/fixtures.rs`: 一時ディレクトリに展開し、本エンジンの出力をゴールデン値とフィールドごとに
  比較します。テスト実行時に git/gitversion は不要です（再現可能）。

## Known simplifications / not implemented

- `track-merge-target`: 上流の `MainlineVersionStrategy` と `GetTaggedSemanticVersion()` でのみ
  消費されるフラグです。この移植版は HEAD から到達可能なすべてのタグをすでに考慮しているため、
  到達可能なマージターゲットのタグはカバーされますが、到達不可能なもの（主に Mainline）は
  カバーされません。
- `/nofetch /nonormalize /allowshallow` は認識されますが、この移植版の構造上、正直なところ
  no-op です（動的クローンが fetch/normalize を直接実行します）。
- `GitVersionInformation` のソースファイル生成は、上流では CLI ではなく MSBuild タスクで処理されるため、
  この CLI 移植版の範囲外です。

検証は本物の GitVersion 6.7.0 バイナリに対する差分テストによって保証されています
（シナリオ × 出力フィールド、5 つのビルドエージェント、ファイル出力）。
