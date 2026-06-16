# gitversion（Rust 移植版）

[English](README.md) · [한국어](README.ko.md) · [日本語](README.ja.md) · **中文**

[GitVersion](https://gitversion.net)（.NET）的 Rust 移植版。它根据你的 Git 历史
计算语义化版本（SemVer）。

> **项目目标：以最小的代价，在没有 .NET 的环境中运行 GitVersion。**
> 一个独立自包含的原生二进制文件——无需 .NET 运行时，无需全局工具安装。
> 纯 Rust 实现的 Git 访问（不依赖 libgit2/C），并针对真实的 GitVersion
> 二进制文件进行差分验证。

CLI、交互式 TUI 以及所有内部消息均通过 [`rust-i18n`](https://github.com/longbridge/rust-i18n)
实现**完整的国际化**（英语 / 韩语 / 日语 / 中文）。默认使用英语；可通过
`--lang ko|ja|zh` 或 `LANG`/`LC_ALL` 环境变量进行覆盖。

## 功能特性

- **纯 Rust 实现的 Git 访问**：[`gix`](https://github.com/GitoxideLabs/gitoxide)（gitoxide）——不依赖 libgit2/C
- **CLI**：[`clap`](https://docs.rs/clap)
- **日志**：[`env_logger`](https://docs.rs/env_logger)（`RUST_LOG`，或 `--verbosity`/`--diag`）
- **i18n**：[`rust-i18n`](https://github.com/longbridge/rust-i18n)，默认英语，`--lang`/`LANG`，区域设置位于 `locales/app.yml`
- **TUI**：[`ratatui`](https://ratatui.rs)（`--tui`）——5 个标签页（Variables/Config/Commits/Branches/Actions）。
  变量搜索与复制、**在 Config 标签页中编辑全局配置**（Enter），即时刷新生效后的
  结果，并**以最小差分的方式保存到 GitVersion.yml**；显示标记了版本来源的
  第一父提交（first-parent），支持按分支重新计算，以及各类操作（创建标签/分支、
  设置 next-version、**切换 Conventional Commits（持久化）**、**编辑/运行 exec 钩子**、保存
  配置、清除缓存、动态克隆、重新计算）。version 钩子会被立即反映。
  panic 会被捕获（`catch_unwind`），终端会被恢复，并优雅退出
- **工作流**：GitFlow / GitHubFlow / TrunkBased（Mainline）
- **版本策略**：ConfiguredNextVersion、TaggedCommit、MergeMessage、VersionInBranchName、
  TrackReleaseBranches、Fallback、Mainline
- **递增约定**：GitVersion 的 `+semver:` 以及 **Conventional Commits**（`feat`→minor、
  `fix`/`perf`→patch、`feat!`/`BREAKING CHANGE:`→major），可通过
  `commit-message-convention: ConventionalCommits` 选择（借鉴自 semantic-release 的方案）
- **部署模式**：ManualDeployment / ContinuousDelivery / ContinuousDeployment
- **输出**：JSON、dot-env、build-server、单个变量（`-v`）、格式化字符串（`--format`）
- **构建代理集成**：TeamCity、Azure Pipelines、GitHub Actions、GitLab CI、Jenkins、
  AppVeyor、TravisCI、Drone、CodeBuild、ContinuaCI、EnvRun、MyGet、BitBucket、BuildKite、
  SpaceAutomation——通过环境自动检测，并以各 CI 的格式输出（`--output build-server`）
- **文件输出**：更新/创建 AssemblyInfo（`--updateassemblyinfo [file] [--ensureassemblyinfo]`）、
  更新项目文件（`--updateprojectfiles`，使用真实的 XML 解析而非正则）、
  Wix 版本文件（`--updatewixversionfile`）
- **包清单**：`--updatepackagefiles` 使用保留格式的解析器（serde_json/toml_edit）
  更新 `package.json`（Node.js）、`Cargo.toml`（Rust）以及 `pyproject.toml`
  （Python，PEP 621/Poetry）中的版本号
- **外部命令钩子（exec）**：类似 semantic-release 的 exec 插件，在生命周期钩子
  （`verify`/`prepare`/`publish`/`success`/`fail`）中运行 shell 命令。版本变量会
  以 `GitVersion_*` 环境变量以及 `{Variable}`/`{env:VAR}` 模板的形式暴露。`version` 钩子
  会根据命令的标准输出修改版本（应用 next-version，然后重新计算）。支持
  `--exec`/`--exec-version`/`--dry-run`
- **结果缓存**：结果存储于 `<.git>/gitversion_cache/<key>.json`。键是
  refs·HEAD·配置文件·overrideconfig 的 SHA1，因此当仓库状态变化时会自动失效。
  可通过 `--nocache` 禁用
- **动态远程仓库**：`--url <repo> --branch <b>` 进行克隆并计算（`-u`/`-p` 认证、
  `-c` 提交、`--dynamicRepoLocation`）。通过 https/file 以及 SSH 进行纯 Rust 的 gix 克隆
  （`ssh://`、类 scp 的 `git@host:path`，使用系统 ssh）
  - **凭据助手 / 操作系统密钥环**：对于 https 认证，它会使用 git 的 credential-helper 协议。
    在没有 `-u`/`-p` 时，它会调用已配置的 `credential.helper`，因此存储在 macOS
    钥匙串（`osxkeychain`）、GCM、libsecret 等中的凭据会被自动使用（完整的 get/erase 协议）

## 构建

```bash
cargo build --release
```

## 用法

```bash
# 将当前仓库的所有变量以 JSON 形式打印
gitversion

# 单个变量
gitversion -v FullSemVer

# 格式化字符串
gitversion --format "v{Major}.{Minor}.{Patch} ({EscapedBranchName})"

# 输出格式
gitversion --output json
gitversion --output dot-env
gitversion --output build-server

# 配置 / 覆盖
gitversion --config GitVersion.yml
gitversion --overrideconfig next-version=2.0.0
gitversion --showconfig

# 外部命令钩子（exec）——版本变量以环境变量/模板形式暴露
gitversion --exec 'npm version {SemVer} --no-git-tag-version'
gitversion --exec-version './scripts/decide-version.sh'
gitversion --exec 'make release' --dry-run

# 交互式 TUI
gitversion --tui

# 语言（默认英语）
gitversion --lang ko
gitversion --lang ja
gitversion --lang zh

# 为指定分支计算
gitversion -b release/2.0.0
```

## 配置文件

在工作目录（以及仓库根目录）中搜索 `GitVersion.yml`、`GitVersion.yaml`、
`.GitVersion.yml`、`.GitVersion.yaml`。键使用与上游 GitVersion 相同的 kebab-case 命名。

```yaml
workflow: GitFlow/v1
next-version: 1.0.0
tag-prefix: "[vV]?"
branches:
  develop:
    increment: Minor
    label: alpha
```

## 项目结构

| 模块 | 职责 | 上游对应 |
|---|---|---|
| `src/git` | 基于 gix 的仓库访问 | `GitVersion.LibGit2Sharp` |
| `src/config` | 配置模型 / 工作流默认值 / 加载器 / 生效配置 | `GitVersion.Configuration` |
| `src/version` | SemanticVersion 及计算引擎 | `GitVersion.Core` |
| `src/output` | 输出变量 / 格式化器 | `GitVersion.Output` |
| `src/cli` | clap 参数 | `GitVersion.App` |
| `src/tui` | ratatui UI | （新增） |
| `src/i18n.rs` + `locales/` | rust-i18n 区域设置处理 | （新增） |

> 注意：`refs/gitversion` 是本移植版所基于的 .NET 源码；它已通过 `.gitignore`
> 排除在版本跟踪之外。

## 测试

使用以真实 GitVersion 6.x 二进制文件作为黄金参考的**差分测试**。

```bash
# 完整测试套件（单元测试 + fixture 集成测试）
cargo test

# 重新生成 fixtures（需要真实的 gitversion 二进制文件）
GITVERSION_BIN=/opt/homebrew/bin/gitversion ./tests/build_fixtures.sh
```

- `tests/build_fixtures.sh`：为每个场景构建 git 仓库，运行真实的 GitVersion 以记录
  黄金 `expected.json`，然后将它们打包进 `testdata/fixtures.tar.gz`。
- `tests/fixtures.rs`：解包到临时目录，并将我们引擎的输出逐字段与黄金值进行
  比较。测试时无需 git/gitversion（可复现）。

## 已知的简化 / 未实现

- `track-merge-target`：一个仅被上游的 `MainlineVersionStrategy` 和
  `GetTaggedSemanticVersion()` 使用的标志。本移植版已经考虑了从 HEAD 可达的所有标签，
  因此可达的 merge-target 标签已被覆盖；不可达的（主要是 Mainline 场景）则未覆盖。
- 日志文件输出（`/l`）未实现。`/nofetch /nonormalize /allowshallow` 会被识别，
  但鉴于本移植版的结构，它们是诚实的空操作（动态克隆直接执行 fetch/normalize）。
- `GitVersionInformation` 源文件生成在上游是由 MSBuild 任务（而非 CLI）处理的，
  因此超出了本 CLI 移植版的范围。

验证通过针对真实 GitVersion 6.7.0 二进制文件的差分测试得到保证
（场景 × 输出字段、5 个构建代理、文件输出）。
