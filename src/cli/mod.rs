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

    /// 로그 상세도.
    #[arg(long, value_enum, default_value = "normal")]
    pub verbosity: Verbosity,

    /// 진단 모드(Trace 로깅).
    #[arg(long)]
    pub diag: bool,

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
