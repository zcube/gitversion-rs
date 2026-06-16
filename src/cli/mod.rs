//! clap 기반 명령줄 인터페이스.
//!
//! 원본 `GitVersion.App/ArgumentParser.cs` 의 주요 옵션을 옮긴다.

use crate::config::{
    DeploymentMode, GitVersionConfiguration, IncrementStrategy, SemanticVersionFormat,
};
use clap::{Parser, ValueEnum};
use std::path::PathBuf;

/// 출력 형식.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    Json,
    DotEnv,
    BuildServer,
}

/// 로그 상세도. 원본 `Verbosity`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Verbosity {
    Quiet,
    Minimal,
    Normal,
    Verbose,
    Diagnostic,
}

impl Verbosity {
    /// log 크레이트 레벨 필터로 변환.
    pub fn to_level(self) -> log::LevelFilter {
        match self {
            Verbosity::Quiet => log::LevelFilter::Error,
            Verbosity::Minimal => log::LevelFilter::Warn,
            Verbosity::Normal => log::LevelFilter::Info,
            Verbosity::Verbose => log::LevelFilter::Debug,
            Verbosity::Diagnostic => log::LevelFilter::Trace,
        }
    }
}

/// GitVersion (Rust 포트).
#[derive(Debug, Parser)]
#[command(name = "gitversion", version, about = "Git 히스토리로부터 의미론적 버전을 계산합니다 (GitVersion Rust 포트)")]
pub struct Cli {
    /// 저장소 경로(`.git` 포함 디렉터리). 생략 시 현재 디렉터리.
    #[arg(default_value = ".")]
    pub path: PathBuf,

    /// path 와 동일하지만 위치 무관(원본 `/targetpath`).
    #[arg(long = "targetpath", value_name = "DIR")]
    pub target_path: Option<PathBuf>,

    // nofetch/nonormalize/allowshallow 는 원본 CLI 호환을 위해 인식하지만, 이 포트는
    // fetch/normalize 를 하지 않으므로 동작상 무효과(no-op)다.
    /// fetch 비활성화(무효과: 본 포트는 fetch 하지 않음).
    #[arg(long)]
    pub nofetch: bool,
    /// 정규화 비활성화(무효과).
    #[arg(long)]
    pub nonormalize: bool,
    /// 디스크 캐시 읽기·쓰기 비활성화(`<.git>/gitversion_cache`).
    #[arg(long)]
    pub nocache: bool,
    /// shallow clone 허용(무효과: gix 가 shallow 도 읽음).
    #[arg(long)]
    pub allowshallow: bool,

    /// 출력 형식(json, dot-env, build-server). 여러 번 지정 가능.
    #[arg(long, value_enum, default_value = "json")]
    pub output: Vec<OutputFormat>,

    /// 출력 파일 경로(지정 시 결과를 파일로 기록).
    #[arg(long = "outputfile")]
    pub output_file: Option<PathBuf>,

    /// 단일 변수만 출력(예: -v SemVer).
    #[arg(long = "showvariable", short = 'v')]
    pub show_variable: Option<String>,

    /// 포맷 문자열로 출력(예: --format "{Major}.{Minor}").
    #[arg(long)]
    pub format: Option<String>,

    /// 설정 파일 경로.
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// 유효 설정을 YAML 로 출력하고 종료.
    #[arg(long = "showconfig")]
    pub show_config: bool,

    /// 인라인 설정 오버라이드(key=value). 여러 번 지정 가능.
    #[arg(long = "overrideconfig")]
    pub override_config: Vec<String>,

    /// 계산 대상 브랜치명(현재 체크아웃 대신).
    #[arg(long, short = 'b')]
    pub branch: Option<String>,

    /// 출력 언어(ko/en/ja/zh). 생략 시 LANG/LC_ALL 환경변수 사용.
    #[arg(long, value_name = "LANG")]
    pub lang: Option<String>,

    /// 로그 상세도.
    #[arg(long, value_enum, default_value = "normal")]
    pub verbosity: Verbosity,

    /// 진단 모드(Trace 로깅).
    #[arg(long)]
    pub diag: bool,

    /// AssemblyInfo 파일 갱신(파일명 생략 시 재귀 탐색).
    #[arg(long = "updateassemblyinfo", num_args = 0.., value_name = "FILE")]
    pub update_assembly_info: Option<Vec<String>>,

    /// AssemblyInfo 파일이 없으면 생성(updateassemblyinfo 와 함께).
    #[arg(long = "ensureassemblyinfo")]
    pub ensure_assembly_info: bool,

    /// 프로젝트 파일(.csproj 등) 버전 요소 갱신(파일명 생략 시 재귀 탐색).
    #[arg(long = "updateprojectfiles", num_args = 0.., value_name = "FILE")]
    pub update_project_files: Option<Vec<String>>,

    /// GitVersion_WixVersion.wxi 생성.
    #[arg(long = "updatewixversionfile")]
    pub update_wix_version_file: bool,

    /// 패키지 매니페스트의 version 갱신(package.json/Cargo.toml/pyproject.toml).
    /// 파일명 생략 시 재귀 탐색.
    #[arg(long = "updatepackagefiles", num_args = 0.., value_name = "FILE")]
    pub update_package_files: Option<Vec<String>>,

    /// 원격 git 저장소 URL(지정 시 clone 후 계산). `--branch` 필수.
    #[arg(long)]
    pub url: Option<String>,

    /// 원격 인증 사용자명(`--url` 과 함께).
    #[arg(long = "username", short = 'u')]
    pub username: Option<String>,

    /// 원격 인증 비밀번호(`--url` 과 함께).
    #[arg(long = "password", short = 'p')]
    pub password: Option<String>,

    /// 확인할 커밋 ID(생략 시 브랜치 최신). `--url` 과 함께.
    #[arg(long = "commit", short = 'c')]
    pub commit: Option<String>,

    /// 동적 clone 위치(기본: 임시 디렉터리).
    #[arg(long = "dynamicRepoLocation")]
    pub dynamic_repo_location: Option<PathBuf>,

    /// 계산 후 실행할 prepare 명령(버전 변수가 GitVersion_* 환경변수와 {Var} 로 노출).
    #[arg(long)]
    pub exec: Option<String>,

    /// 버전 수정 명령. 표준출력을 next-version 으로 적용해 재계산한다.
    #[arg(long = "exec-version")]
    pub exec_version: Option<String>,

    /// exec 훅을 실제 실행하지 않고 출력만 한다.
    #[arg(long = "dry-run")]
    pub dry_run: bool,

    /// 대화형 Ratatui TUI 실행.
    #[arg(long)]
    pub tui: bool,
}

/// `key=value` 오버라이드를 설정에 적용.
pub fn apply_overrides(config: &mut GitVersionConfiguration, overrides: &[String]) {
    for raw in overrides {
        let Some((key, value)) = raw.split_once('=') else {
            log::warn!("잘못된 overrideconfig 항목(무시): {raw}");
            continue;
        };
        let key = key.trim();
        let value = value.trim().to_string();
        match key {
            "tag-prefix" => config.tag_prefix = Some(value),
            "next-version" => config.next_version = Some(value),
            "label" => config.label = Some(value),
            "commit-date-format" => config.commit_date_format = Some(value),
            "major-version-bump-message" => config.major_version_bump_message = Some(value),
            "minor-version-bump-message" => config.minor_version_bump_message = Some(value),
            "patch-version-bump-message" => config.patch_version_bump_message = Some(value),
            "no-bump-message" => config.no_bump_message = Some(value),
            "tag-pre-release-weight" => {
                if let Ok(n) = value.parse() {
                    config.tag_pre_release_weight = Some(n);
                }
            }
            "update-build-number" => config.update_build_number = value.parse().ok(),
            "increment" => config.increment = parse_increment(&value),
            "mode" => config.mode = parse_mode(&value),
            "semantic-version-format" => {
                config.semantic_version_format = match value.to_lowercase().as_str() {
                    "loose" => Some(SemanticVersionFormat::Loose),
                    _ => Some(SemanticVersionFormat::Strict),
                }
            }
            "commit-message-convention" => {
                config.commit_message_convention = match value.to_lowercase().replace('-', "").as_str() {
                    "conventionalcommits" | "conventional" => {
                        Some(crate::config::CommitMessageConvention::ConventionalCommits)
                    }
                    _ => Some(crate::config::CommitMessageConvention::Default),
                }
            }
            other => log::warn!("지원하지 않는 overrideconfig 키(무시): {other}"),
        }
    }
}

fn parse_increment(v: &str) -> Option<IncrementStrategy> {
    Some(match v.to_lowercase().as_str() {
        "major" => IncrementStrategy::Major,
        "minor" => IncrementStrategy::Minor,
        "patch" => IncrementStrategy::Patch,
        "none" => IncrementStrategy::None,
        "inherit" => IncrementStrategy::Inherit,
        _ => return None,
    })
}

fn parse_mode(v: &str) -> Option<DeploymentMode> {
    Some(match v.to_lowercase().as_str() {
        "continuousdelivery" => DeploymentMode::ContinuousDelivery,
        "continuousdeployment" => DeploymentMode::ContinuousDeployment,
        "manualdeployment" => DeploymentMode::ManualDeployment,
        _ => return None,
    })
}
