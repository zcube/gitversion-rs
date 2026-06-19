//! Application entry logic (called from the binary). Lives inside the lib so the `t!` macro is available.

use crate::cli::{Cli, OutputFormat};
use crate::{buildagent, cache, cli, config, exec, git, i18n, output, remote, tui, version};
use anyhow::{Context, Result};
use clap::FromArgMatches;
use rust_i18n::t;
use std::io::Write;
use std::path::PathBuf;

/// Detect the locale from `--lang` or environment variables before clap parsing, so that
/// `--help` and `--version` output also respects the locale. Recognises both `--lang ko` and `--lang=ko`.
fn pre_detect_lang(raw: &[String]) -> Option<String> {
    let mut it = raw.iter();
    while let Some(a) = it.next() {
        if let Some(v) = a.strip_prefix("--lang=") {
            return Some(v.to_string());
        }
        if a == "--lang" {
            return it.next().cloned();
        }
    }
    None
}

/// Binary entry point: run the application and print an error message then exit on failure.
pub fn main() {
    if let Err(e) = run() {
        eprintln!("{}", t!("error.generic", error = format!("{e:#}")));
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    // Set the locale before parsing so --help/--version output also uses it.
    let raw: Vec<String> = std::env::args().collect();
    i18n::init(pre_detect_lang(&raw).as_deref());

    // Parse with the localised help/about text (--help/--version exits here).
    let matches = cli::localized_command().get_matches();
    let args = Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());

    // Logging: RUST_LOG takes priority; otherwise derive the level from --verbosity / --diag.
    let level = if args.diag {
        log::LevelFilter::Trace
    } else {
        args.verbosity.to_level()
    };
    // Log destination: file (append) with --log <FILE>, stderr with --log console,
    // or stderr by default. stdout is always reserved for the version output so that
    // `$(gitversion ...)` captures stay clean.
    let mut builder = env_logger::Builder::new();
    builder.filter_level(level).parse_default_env();
    match &args.log_file {
        // Mirrors the original GitVersion `/l console`: log to the console (stderr) rather than a file.
        Some(path) if path.as_os_str().eq_ignore_ascii_case("console") => {
            builder
                .format_timestamp(None)
                .target(env_logger::Target::Stderr);
        }
        Some(path) => {
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .with_context(|| t!("error.log_open", path = path.display()))?;
            // Include timestamps in file logs (matching the character of the original GitVersion log files).
            builder.target(env_logger::Target::Pipe(Box::new(file)));
        }
        None => {
            builder
                .format_timestamp(None)
                .target(env_logger::Target::Stderr);
        }
    }
    builder.init();

    // If --url is given, clone the remote repository dynamically and use the clone path as the target.
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
        args.target_path
            .clone()
            .unwrap_or_else(|| args.path.clone())
    };
    log::debug!("target path: {}", target.display());

    let repo = git::GitRepo::discover(&target).with_context(|| t!("error.git_open").to_string())?;
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

    // Cache key inputs: overrideconfig values + branch override.
    let mut key_inputs = args.override_config.clone();
    if let Some(b) = &args.branch {
        key_inputs.push(format!("branch={b}"));
    }
    let config_path = args
        .config
        .clone()
        .or_else(|| config::loader::locate(&work_dir, repo_root.as_deref()));
    let cache_key = if args.nocache {
        None
    } else {
        Some(cache::compute_key(
            &repo,
            config_path.as_deref(),
            &key_inputs,
        ))
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

    // version hook: modify the version via external command output and recalculate.
    let version_cmd = args
        .exec_version
        .clone()
        .or_else(|| configuration.exec.get("version").cloned());
    if let Some(cmd) = version_cmd {
        if let Some(new_ver) = exec::run_version_hook(&cmd, &variables, &work_dir, args.dry_run)? {
            log::info!("{}", t!("log.version_hook_modified", ver = new_ver));
            configuration.next_version = Some(new_ver);
            variables = version::calculation::calculate(&repo, &configuration, args.branch.clone())
                .with_context(|| t!("error.version_hook_recalc").to_string())?;
        }
    }

    // File output.
    if let Some(files) = &args.update_assembly_info {
        for p in output::files::update_assembly_info(
            &variables,
            &work_dir,
            files,
            args.ensure_assembly_info,
        )? {
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

    // External command hooks.
    if !configuration.exec.is_empty() || args.exec.is_some() {
        exec::run_hooks(
            &configuration.exec,
            args.exec.as_deref(),
            &variables,
            &work_dir,
            args.dry_run,
        )?;
    }

    // Single variable / format string.
    if let Some(name) = &args.show_variable {
        return emit(&args, output::generator::show_variable(&variables, name)?);
    }
    if let Some(template) = &args.format {
        return emit(
            &args,
            output::generator::format_template(&variables, template)?,
        );
    }

    // Output format rendering.
    let mut rendered = String::new();
    for (i, fmt) in args.output.iter().enumerate() {
        if i > 0 {
            rendered.push('\n');
        }
        match fmt {
            // `File` mirrors the original `/output file`: rendered as JSON, then written to --outputfile by `emit`.
            OutputFormat::Json | OutputFormat::File => {
                rendered.push_str(&output::generator::to_json(&variables)?)
            }
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

/// Write the result to a file or to stdout.
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
