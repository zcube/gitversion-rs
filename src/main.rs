//! GitVersion (Rust 포트) 진입점.
//!
//! - CLI: clap
//! - 로깅: env_logger (RUST_LOG 또는 --verbosity/--diag)
//! - Git: gix (순수 Rust)
//! - TUI: ratatui

use anyhow::{Context, Result};
use clap::Parser;
use gitversion::{buildagent, cli, config, git, output, tui, version};
use cli::{Cli, OutputFormat};
use std::io::Write;
use std::path::PathBuf;

fn main() {
    if let Err(e) = run() {
        eprintln!("오류: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = Cli::parse();

    // 로깅 초기화: RUST_LOG 가 있으면 우선, 없으면 verbosity/diag 기반.
    let level = if args.diag { log::LevelFilter::Trace } else { args.verbosity.to_level() };
    env_logger::Builder::new()
        .filter_level(level)
        .parse_default_env()
        .format_timestamp(None)
        .init();

    log::debug!("대상 경로: {}", args.path.display());

    // 저장소 오픈.
    let repo = git::GitRepo::discover(&args.path)
        .context("git 저장소를 열 수 없습니다 (먼저 'git init' 후 커밋이 필요합니다)")?;
    let work_dir = args.path.canonicalize().unwrap_or_else(|_| args.path.clone());
    let repo_root: Option<PathBuf> = repo.workdir().map(|p| p.to_path_buf());

    // 설정 로드 + 오버라이드.
    let mut configuration =
        config::loader::load(args.config.as_deref(), &work_dir, repo_root.as_deref())?;
    cli::apply_overrides(&mut configuration, &args.override_config);

    if args.show_config {
        let yaml = serde_yaml::to_string(&configuration)?;
        println!("{yaml}");
        return Ok(());
    }

    // 버전 계산.
    let variables = version::calculation::calculate(&repo, &configuration, args.branch.clone())
        .context("버전 계산에 실패했습니다")?;

    let branch_name = match &args.branch {
        Some(b) => b.clone(),
        None => repo.current_branch_name().unwrap_or_default(),
    };

    // 파일 출력 작업(AssemblyInfo / 프로젝트 파일 / Wix).
    if let Some(files) = &args.update_assembly_info {
        let updated = output::files::update_assembly_info(
            &variables,
            &work_dir,
            files,
            args.ensure_assembly_info,
        )?;
        for p in &updated {
            log::info!("AssemblyInfo 갱신: {}", p.display());
        }
    }
    if let Some(files) = &args.update_project_files {
        let updated = output::files::update_project_files(&variables, &work_dir, files)?;
        for p in &updated {
            log::info!("프로젝트 파일 갱신: {}", p.display());
        }
    }
    if args.update_wix_version_file {
        let p = output::files::write_wix(&variables, &work_dir)?;
        log::info!("Wix 버전 파일 생성: {}", p.display());
    }

    // TUI 모드.
    if args.tui {
        return tui::run(variables, branch_name);
    }

    // 단일 변수.
    if let Some(name) = &args.show_variable {
        let value = output::generator::show_variable(&variables, name)?;
        emit(&args, value)?;
        return Ok(());
    }

    // 포맷 문자열.
    if let Some(template) = &args.format {
        let value = output::generator::format_template(&variables, template)?;
        emit(&args, value)?;
        return Ok(());
    }

    // 출력 형식.
    let mut rendered = String::new();
    for (i, fmt) in args.output.iter().enumerate() {
        if i > 0 {
            rendered.push('\n');
        }
        match fmt {
            OutputFormat::Json => rendered.push_str(&output::generator::to_json(&variables)?),
            OutputFormat::DotEnv => rendered.push_str(&output::generator::to_dotenv(&variables)),
            OutputFormat::BuildServer => {
                let update_build_number = configuration.update_build_number.unwrap_or(true);
                match buildagent::detect() {
                    Some(agent) => {
                        log::info!("감지된 빌드에이전트: {}", agent.name());
                        let lines = agent.write_integration(&variables, update_build_number);
                        rendered.push_str(&lines.join("\n"));
                    }
                    None => {
                        // 에이전트 미감지 시 GitVersion_K=V 형식으로 출력.
                        rendered.push_str(&output::generator::to_buildserver_env(&variables));
                    }
                }
            }
        }
    }
    emit(&args, rendered)?;
    Ok(())
}

/// 결과를 파일 또는 stdout 으로 출력.
fn emit(args: &Cli, content: String) -> Result<()> {
    if let Some(path) = &args.output_file {
        let mut f = std::fs::File::create(path)
            .with_context(|| format!("출력 파일 생성 실패: {}", path.display()))?;
        f.write_all(content.as_bytes())?;
        if !content.ends_with('\n') {
            f.write_all(b"\n")?;
        }
        log::info!("결과를 {} 에 기록했습니다", path.display());
    } else {
        println!("{content}");
    }
    Ok(())
}
