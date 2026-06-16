//! 애플리케이션 진입 로직(바이너리에서 호출). t! 매크로를 쓰기 위해 lib 안에 둔다.

use crate::cli::{Cli, OutputFormat};
use crate::{buildagent, cache, cli, config, exec, git, i18n, output, remote, tui, version};
use anyhow::{Context, Result};
use clap::Parser;
use rust_i18n::t;
use std::io::Write;
use std::path::PathBuf;

/// 바이너리 main: 실행하고 에러 시 메시지 출력 후 종료.
pub fn main() {
    if let Err(e) = run() {
        eprintln!("{}", t!("error.generic", error = format!("{e:#}")));
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = Cli::parse();

    // 로케일 초기화(--lang 우선, 없으면 LANG/LC_ALL, 기본 영어).
    i18n::init(args.lang.as_deref());

    // 로깅 초기화: RUST_LOG 가 있으면 우선, 없으면 verbosity/diag 기반.
    let level = if args.diag { log::LevelFilter::Trace } else { args.verbosity.to_level() };
    env_logger::Builder::new()
        .filter_level(level)
        .parse_default_env()
        .format_timestamp(None)
        .init();

    // --url 이 주어지면 원격 저장소를 동적으로 clone 해 그 경로를 대상으로 사용.
    let target = if let Some(url) = &args.url {
        let opts = remote::DynamicRepoOptions {
            url: url.clone(),
            branch: args.branch.clone(),
            username: args.username.clone(),
            password: args.password.clone(),
            commit: args.commit.clone(),
            location: args.dynamic_repo_location.clone(),
        };
        remote::prepare(&opts).with_context(|| t!("error.dynamic_repo").to_string())?
    } else {
        args.target_path.clone().unwrap_or_else(|| args.path.clone())
    };
    log::debug!("target path: {}", target.display());

    let repo = git::GitRepo::discover(&target)
        .with_context(|| t!("error.git_open").to_string())?;
    let work_dir = target.canonicalize().unwrap_or(target);
    let repo_root: Option<PathBuf> = repo.workdir().map(|p| p.to_path_buf());

    let mut configuration =
        config::loader::load(args.config.as_deref(), &work_dir, repo_root.as_deref())?;
    cli::apply_overrides(&mut configuration, &args.override_config);

    if args.show_config {
        println!("{}", serde_yaml::to_string(&configuration)?);
        return Ok(());
    }

    if args.tui {
        return tui::run(repo, configuration, work_dir);
    }

    // 캐시 키 입력: overrideconfig + 브랜치 오버라이드.
    let mut key_inputs = args.override_config.clone();
    if let Some(b) = &args.branch {
        key_inputs.push(format!("branch={b}"));
    }
    let config_path =
        args.config.clone().or_else(|| config::loader::locate(&work_dir, repo_root.as_deref()));
    let cache_key = if args.nocache {
        None
    } else {
        Some(cache::compute_key(&repo, config_path.as_deref(), &key_inputs))
    };

    let mut variables = match cache_key.as_deref().and_then(|k| cache::load(&repo, k)) {
        Some(v) => v,
        None => {
            let v = version::calculation::calculate(&repo, &configuration, args.branch.clone())
                .with_context(|| t!("error.calc_failed").to_string())?;
            if let Some(k) = &cache_key {
                cache::store(&repo, k, &v);
            }
            v
        }
    };

    // version 훅: 외부 명령 출력으로 버전 정보를 수정하고 재계산.
    let version_cmd =
        args.exec_version.clone().or_else(|| configuration.exec.get("version").cloned());
    if let Some(cmd) = version_cmd {
        if let Some(new_ver) = exec::run_version_hook(&cmd, &variables, &work_dir, args.dry_run)? {
            log::info!("{}", t!("log.version_hook_modified", ver = new_ver));
            configuration.next_version = Some(new_ver);
            variables = version::calculation::calculate(&repo, &configuration, args.branch.clone())
                .with_context(|| t!("error.version_hook_recalc").to_string())?;
        }
    }

    // 파일 출력.
    if let Some(files) = &args.update_assembly_info {
        for p in output::files::update_assembly_info(&variables, &work_dir, files, args.ensure_assembly_info)? {
            log::info!("{}", t!("log.assemblyinfo_updated", path = p.display()));
        }
    }
    if let Some(files) = &args.update_project_files {
        for p in output::files::update_project_files(&variables, &work_dir, files)? {
            log::info!("{}", t!("log.projectfile_updated", path = p.display()));
        }
    }
    if args.update_wix_version_file {
        let p = output::files::write_wix(&variables, &work_dir)?;
        log::info!("{}", t!("log.wix_created", path = p.display()));
    }
    if let Some(files) = &args.update_package_files {
        for p in output::files::update_package_files(&variables, &work_dir, files)? {
            log::info!("{}", t!("log.package_updated", path = p.display()));
        }
    }

    // 외부 명령 훅.
    if !configuration.exec.is_empty() || args.exec.is_some() {
        exec::run_hooks(&configuration.exec, args.exec.as_deref(), &variables, &work_dir, args.dry_run)?;
    }

    // 단일 변수 / 포맷 문자열.
    if let Some(name) = &args.show_variable {
        return emit(&args, output::generator::show_variable(&variables, name)?);
    }
    if let Some(template) = &args.format {
        return emit(&args, output::generator::format_template(&variables, template)?);
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
            OutputFormat::BuildServer => match buildagent::detect() {
                Some(agent) => {
                    log::info!("{}", t!("log.agent_detected", name = agent.name()));
                    let ubn = configuration.update_build_number.unwrap_or(true);
                    rendered.push_str(&agent.write_integration(&variables, ubn).join("\n"));
                }
                None => rendered.push_str(&output::generator::to_buildserver_env(&variables)),
            },
        }
    }
    emit(&args, rendered)
}

/// 결과를 파일 또는 stdout 으로 출력.
fn emit(args: &Cli, content: String) -> Result<()> {
    if let Some(path) = &args.output_file {
        let mut f = std::fs::File::create(path)
            .with_context(|| t!("error.output_file", path = path.display()).to_string())?;
        f.write_all(content.as_bytes())?;
        if !content.ends_with('\n') {
            f.write_all(b"\n")?;
        }
        log::info!("{}", t!("log.result_written", path = path.display()));
    } else {
        println!("{content}");
    }
    Ok(())
}
