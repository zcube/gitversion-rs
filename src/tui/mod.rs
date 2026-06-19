//! Ratatui-based interactive TUI.
//!
//! Goes beyond a simple viewer to offer exploration of computed results and operations
//! that affect the actual version (tag/branch creation, next-version override, cache
//! clearing, dynamic clone, per-branch recomputation).

use crate::config::effective::EffectiveConfiguration;
use crate::config::{loader, GitVersionConfiguration};
use crate::exec;
use crate::git::{CommitInfo, GitRepo};
use crate::output::{generator, VersionVariables};
use crate::remote::{self, DynamicRepoOptions};
use crate::version::calculation;
use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Cell, Clear, List, ListItem, Paragraph, Row, Table, Tabs},
};
use rust_i18n::t;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

/// Translation keys for tab titles (resolved via `t!` at render time).
const TAB_KEYS: [&str; 5] = [
    "tui.tab.variables",
    "tui.tab.config",
    "tui.tab.commits",
    "tui.tab.branches",
    "tui.tab.actions",
];

/// Actions that require text input.
#[derive(Clone, Copy, PartialEq)]
enum InputAction {
    CreateTag,
    CreateBranch,
    SetNextVersion,
    DynamicClone,
    EditExecHook,
    EditConfig,
}

impl InputAction {
    fn prompt(&self) -> &'static str {
        match self {
            InputAction::CreateTag => "tui.prompt.create_tag",
            InputAction::CreateBranch => "tui.prompt.create_branch",
            InputAction::SetNextVersion => "tui.prompt.set_next_version",
            InputAction::DynamicClone => "tui.prompt.dynamic_clone",
            InputAction::EditExecHook => "tui.prompt.edit_exec_hook",
            InputAction::EditConfig => "tui.prompt.edit_config",
        }
    }
}

struct App {
    repo: GitRepo,
    config: GitVersionConfiguration,
    work_dir: PathBuf,
    /// Currently checked-out branch (baseline).
    base_branch: String,
    /// Branch to recompute for (selected in the Branches tab). None means the baseline branch.
    branch_override: Option<String>,
    next_version_override: Option<String>,

    vars: VersionVariables,
    json: String,
    commits: Vec<CommitInfo>,
    branches: Vec<String>,

    tab: usize,
    selected: usize,
    scroll: u16,
    search: String,
    searching: bool,
    input: Option<InputAction>,
    input_buf: String,
    status: String,
    actions: Vec<&'static str>,
    /// Signal to the event loop to leave the terminal temporarily and run side-effect hooks.
    pending_run_hooks: bool,
    /// Global config key currently being edited (for EditConfig input).
    edit_config_key: Option<String>,
    /// Global config changes made via the TUI (key=value). Written as a minimal diff when saving.
    tui_overrides: std::collections::BTreeMap<String, String>,
}

/// Global config keys editable in the Config tab (same meaning as overrideconfig).
/// Tuple of (config key, hint translation key); hints are resolved via `t!` at render time.
const EDITABLE_CONFIG: [(&str, &str); 13] = [
    ("increment", "tui.hint.increment"),
    ("mode", "tui.hint.mode"),
    ("label", "tui.hint.prerelease_label"),
    ("tag-prefix", "tui.hint.tag_prefix"),
    ("next-version", "tui.hint.version_example"),
    ("semantic-version-format", "tui.hint.semver_format"),
    ("tag-pre-release-weight", "tui.hint.integer"),
    ("update-build-number", "tui.hint.bool"),
    ("commit-date-format", "tui.hint.date_example"),
    ("major-version-bump-message", "tui.hint.regex"),
    ("minor-version-bump-message", "tui.hint.regex"),
    ("patch-version-bump-message", "tui.hint.regex"),
    ("no-bump-message", "tui.hint.regex"),
];

/// Convert a string to a YAML scalar (bool / int / string).
fn yaml_scalar(v: &str) -> serde_yaml::Value {
    if let Ok(b) = v.parse::<bool>() {
        return serde_yaml::Value::Bool(b);
    }
    if let Ok(i) = v.parse::<i64>() {
        return serde_yaml::Value::Number(i.into());
    }
    serde_yaml::Value::String(v.to_string())
}

/// Current value of a global config key as a string.
fn global_value(config: &GitVersionConfiguration, key: &str) -> String {
    match key {
        "increment" => config
            .increment
            .map(|v| format!("{v:?}"))
            .unwrap_or_default(),
        "mode" => config.mode.map(|v| format!("{v:?}")).unwrap_or_default(),
        "label" => config.label.clone().unwrap_or_default(),
        "tag-prefix" => config.tag_prefix.clone().unwrap_or_default(),
        "next-version" => config.next_version.clone().unwrap_or_default(),
        "semantic-version-format" => config
            .semantic_version_format
            .map(|v| format!("{v:?}"))
            .unwrap_or_default(),
        "tag-pre-release-weight" => config
            .tag_pre_release_weight
            .map(|v| v.to_string())
            .unwrap_or_default(),
        "update-build-number" => config
            .update_build_number
            .map(|v| v.to_string())
            .unwrap_or_default(),
        "commit-date-format" => config.commit_date_format.clone().unwrap_or_default(),
        "major-version-bump-message" => config
            .major_version_bump_message
            .clone()
            .unwrap_or_default(),
        "minor-version-bump-message" => config
            .minor_version_bump_message
            .clone()
            .unwrap_or_default(),
        "patch-version-bump-message" => config
            .patch_version_bump_message
            .clone()
            .unwrap_or_default(),
        "no-bump-message" => config.no_bump_message.clone().unwrap_or_default(),
        _ => String::new(),
    }
}

/// Launch the TUI. Accepts a repository and configuration and runs interactively.
pub fn run(repo: GitRepo, config: GitVersionConfiguration, work_dir: PathBuf) -> Result<()> {
    let base_branch = repo.current_branch_name().unwrap_or_default();
    let mut app = App {
        repo,
        config,
        work_dir,
        base_branch,
        branch_override: None,
        next_version_override: None,
        vars: VersionVariables::default(),
        json: String::new(),
        commits: Vec::new(),
        branches: Vec::new(),
        tab: 0,
        selected: 0,
        scroll: 0,
        search: String::new(),
        searching: false,
        input: None,
        input_buf: String::new(),
        status: t!("tui.status.ready").to_string(),
        // Action label translation keys (resolved via `t!` at render time). Order matches run_action indices.
        actions: vec![
            "tui.action.create_tag",
            "tui.action.create_branch",
            "tui.action.set_next_version",
            "tui.action.edit_exec_hook",
            "tui.action.run_exec_hook",
            "tui.action.save_config",
            "tui.action.clear_cache",
            "tui.action.dynamic_clone",
            "tui.action.recompute",
            "tui.action.reset_base",
        ],
        pending_run_hooks: false,
        edit_config_key: None,
        tui_overrides: std::collections::BTreeMap::new(),
    };
    app.recompute();
    app.reload_lists();

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Replace the panic hook temporarily so that panics do not corrupt the alternate screen;
    // capture the message, then catch_unwind to shut down gracefully.
    let panic_msg: std::sync::Arc<std::sync::Mutex<Option<String>>> = Default::default();
    let captured = panic_msg.clone();
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let msg = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| t!("tui.panic.unknown").to_string());
        let loc = info
            .location()
            .map(|l| format!(" ({}:{})", l.file(), l.line()))
            .unwrap_or_default();
        *captured.lock().unwrap() = Some(format!("{msg}{loc}"));
    }));

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        event_loop(&mut terminal, &mut app)
    }));

    // Always restore the terminal regardless of what happened.
    let _ = disable_raw_mode();
    let _ = execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    );
    let _ = terminal.show_cursor();
    std::panic::set_hook(original_hook);

    match result {
        Ok(r) => r,
        Err(_) => {
            let msg = panic_msg
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| t!("tui.panic.internal").to_string());
            // Convert the panic to a normal error instead of a crash (terminal already restored).
            log::error!("{}", t!("tui.panic.defended", msg = msg));
            Err(anyhow::anyhow!("{}", t!("tui.panic.exit", msg = msg)))
        }
    }
}

impl App {
    /// Recompute with the configuration reflecting the current overrides.
    fn recompute(&mut self) {
        let mut cfg = self.config.clone();
        if let Some(nv) = &self.next_version_override {
            cfg.next_version = Some(nv.clone());
        }
        match calculation::calculate(&self.repo, &cfg, self.branch_override.clone()) {
            Ok(mut v) => {
                let mut hook_applied = false;
                // When a version exec hook is present, use its output to override the version and recompute (mirrors CLI).
                if let Some(cmd) = cfg.exec.get("version").cloned() {
                    if let Ok(Some(nv)) = exec::run_version_hook(&cmd, &v, &self.work_dir, false) {
                        cfg.next_version = Some(nv.clone());
                        if let Ok(v2) =
                            calculation::calculate(&self.repo, &cfg, self.branch_override.clone())
                        {
                            v = v2;
                            self.status =
                                t!("tui.status.version_hook_applied", nv = nv).to_string();
                            hook_applied = true;
                        }
                    }
                }
                self.json = generator::to_json(&v).unwrap_or_default();
                self.vars = v;
                if !hook_applied {
                    self.status = t!(
                        "tui.status.recompute_done",
                        branch = self.branch_override.as_deref().unwrap_or(&self.base_branch)
                    )
                    .to_string();
                }
            }
            Err(e) => self.status = t!("tui.status.calc_error", error = format!("{e}")).to_string(),
        }
        // Refresh the commit list for the target branch.
        let target = self
            .branch_override
            .clone()
            .unwrap_or_else(|| self.base_branch.clone());
        self.commits = self
            .repo
            .first_parent_between(None, &target)
            .unwrap_or_default();
        self.commits.truncate(200);
    }

    fn reload_lists(&mut self) {
        self.branches = self.repo.local_branch_names().unwrap_or_default();
    }

    /// Currently displayed variables with the search filter applied.
    fn filtered_vars(&self) -> Vec<(String, String)> {
        let q = self.search.to_lowercase();
        self.vars
            .to_map()
            .into_iter()
            .filter(|(k, v)| {
                q.is_empty() || k.to_lowercase().contains(&q) || v.to_lowercase().contains(&q)
            })
            .collect()
    }

    fn copy(&mut self, text: &str) {
        match arboard::Clipboard::new().and_then(|mut c| c.set_text(text.to_string())) {
            Ok(_) => self.status = t!("tui.status.copied", text = truncate(text, 40)).to_string(),
            Err(e) => {
                self.status = t!("tui.status.clipboard_failed", error = format!("{e}")).to_string()
            }
        }
    }

    fn confirm_input(&mut self) {
        let action = self.input.take();
        let buf = std::mem::take(&mut self.input_buf);
        let buf = buf.trim().to_string();
        if buf.is_empty() {
            self.status = t!("tui.status.input_cancelled").to_string();
            return;
        }
        match action {
            Some(InputAction::CreateTag) => match self.repo.create_tag(&buf, None) {
                Ok(_) => {
                    self.status = t!("tui.status.tag_created", name = buf).to_string();
                    self.recompute();
                    self.reload_lists();
                }
                Err(e) => {
                    self.status = t!("git.tag_create_failed", name = format!("{e}")).to_string()
                }
            },
            Some(InputAction::CreateBranch) => match self.repo.create_branch(&buf, None) {
                Ok(_) => {
                    self.status = t!("tui.status.branch_created", name = buf).to_string();
                    self.reload_lists();
                }
                Err(e) => {
                    self.status = t!("git.branch_create_failed", name = format!("{e}")).to_string()
                }
            },
            Some(InputAction::SetNextVersion) => {
                self.next_version_override = Some(buf.clone());
                self.status = t!("tui.status.next_version_set", version = buf).to_string();
                self.recompute();
            }
            Some(InputAction::DynamicClone) => self.do_dynamic_clone(&buf),
            Some(InputAction::EditExecHook) => {
                let Some((name, cmd)) = buf.split_once('=') else {
                    self.status = t!("tui.status.format_name_cmd").to_string();
                    return;
                };
                let (name, cmd) = (name.trim().to_string(), cmd.trim().to_string());
                const VALID: [&str; 6] =
                    ["verify", "prepare", "publish", "success", "fail", "version"];
                if !VALID.contains(&name.as_str()) {
                    self.status = t!("tui.status.hook_unknown_name", name = name).to_string();
                    return;
                }
                if cmd.is_empty() {
                    self.config.exec.remove(&name);
                    self.status = t!("tui.status.hook_removed", name = name).to_string();
                } else {
                    self.config.exec.insert(name.clone(), cmd);
                    self.status = t!("tui.status.hook_set", name = name).to_string();
                }
                // A version-hook change affects the version output → recompute then persist.
                self.recompute();
                self.save_config();
            }
            Some(InputAction::EditConfig) => {
                if let Some(key) = self.edit_config_key.take() {
                    self.apply_global_edit(&key, &buf);
                    self.status =
                        t!("tui.status.config_saved_key", key = key, value = buf).to_string();
                }
            }
            None => {}
        }
    }

    fn do_dynamic_clone(&mut self, spec: &str) {
        let mut parts = spec.split_whitespace();
        let (url, branch) = (parts.next(), parts.next());
        let Some(url) = url else {
            self.status = t!("tui.status.url_required").to_string();
            return;
        };
        let opts = DynamicRepoOptions {
            url: url.to_string(),
            branch: branch
                .map(|s| s.to_string())
                .or_else(|| Some("main".into())),
            username: None,
            password: None,
            commit: None,
            location: None,
        };
        self.status = t!("tui.status.cloning").to_string();
        match remote::prepare(&opts) {
            Ok(dest) => match GitRepo::discover(&dest) {
                Ok(repo) => {
                    let root = repo
                        .workdir()
                        .map(|p| p.to_path_buf())
                        .unwrap_or_else(|| dest.clone());
                    self.config = loader::load(None, &root, Some(&root))
                        .unwrap_or_else(|_| self.config.clone());
                    self.repo = repo;
                    self.work_dir = root;
                    self.base_branch = self.repo.current_branch_name().unwrap_or_default();
                    self.branch_override = None;
                    self.next_version_override = None;
                    self.recompute();
                    self.reload_lists();
                    self.status = t!("tui.status.clone_done", url = url).to_string();
                }
                Err(e) => {
                    self.status =
                        t!("tui.status.clone_open_failed", error = format!("{e}")).to_string()
                }
            },
            Err(e) => {
                self.status = t!("tui.status.clone_failed", error = format!("{e}")).to_string()
            }
        }
    }

    fn run_action(&mut self, idx: usize) {
        match idx {
            0 => self.start_input(InputAction::CreateTag),
            1 => self.start_input(InputAction::CreateBranch),
            2 => self.start_input(InputAction::SetNextVersion),
            3 => self.start_input(InputAction::EditExecHook),
            4 => self.pending_run_hooks = true, // Event loop exits the terminal to run.
            5 => self.save_config(),
            6 => match self.repo.clear_cache() {
                Ok(n) => self.status = t!("tui.status.cache_cleared", count = n).to_string(),
                Err(e) => {
                    self.status =
                        t!("tui.status.cache_clear_failed", error = format!("{e}")).to_string()
                }
            },
            7 => self.start_input(InputAction::DynamicClone),
            8 => self.recompute(),
            9 => {
                self.branch_override = None;
                self.next_version_override = None;
                self.recompute();
                self.status =
                    t!("tui.status.reset_base", branch = self.base_branch.clone()).to_string();
            }
            _ => {}
        }
    }

    /// Save changed global config keys to GitVersion.yml as a minimal diff (existing content preserved).
    fn save_config(&mut self) {
        let path = self.work_dir.join("GitVersion.yml");
        let mut doc: serde_yaml::Mapping = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_yaml::from_str(&s).ok())
            .unwrap_or_default();

        // Write the changed global keys as scalars.
        for (k, v) in &self.tui_overrides {
            doc.insert(serde_yaml::Value::String(k.clone()), yaml_scalar(v));
        }
        // Write the exec hook map.
        if self.config.exec.is_empty() {
            doc.remove(serde_yaml::Value::String("exec".into()));
        } else {
            let mut exec_map = serde_yaml::Mapping::new();
            for (k, v) in &self.config.exec {
                exec_map.insert(
                    serde_yaml::Value::String(k.clone()),
                    serde_yaml::Value::String(v.clone()),
                );
            }
            doc.insert(
                serde_yaml::Value::String("exec".into()),
                serde_yaml::Value::Mapping(exec_map),
            );
        }

        match serde_yaml::to_string(&doc)
            .map_err(anyhow::Error::from)
            .and_then(|y| std::fs::write(&path, y).map_err(anyhow::Error::from))
        {
            Ok(_) => self.status = t!("tui.status.config_saved", path = path.display()).to_string(),
            Err(e) => {
                self.status =
                    t!("tui.status.config_save_failed", error = format!("{e}")).to_string()
            }
        }
    }

    /// Edit a global config key (same logic as overrideconfig), record it, recompute, and save.
    fn apply_global_edit(&mut self, key: &str, value: &str) {
        crate::cli::apply_overrides(&mut self.config, &[format!("{key}={value}")]);
        self.tui_overrides
            .insert(key.to_string(), value.to_string());
        self.recompute();
        self.save_config();
    }

    fn start_input(&mut self, action: InputAction) {
        self.input = Some(action);
        self.input_buf.clear();
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}

fn event_loop<B: Backend>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()>
where
    B::Error: std::error::Error + Send + Sync + 'static,
{
    loop {
        terminal.draw(|f| ui(f, app))?;
        if !event::poll(Duration::from_millis(200))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }

        // Input modal takes priority.
        if app.input.is_some() {
            match key.code {
                KeyCode::Esc => {
                    app.input = None;
                    app.input_buf.clear();
                }
                KeyCode::Enter => app.confirm_input(),
                KeyCode::Backspace => {
                    app.input_buf.pop();
                }
                KeyCode::Char(c) => app.input_buf.push(c),
                _ => {}
            }
            continue;
        }

        // Search input mode (Variables tab).
        if app.searching {
            match key.code {
                KeyCode::Esc => {
                    app.searching = false;
                    app.search.clear();
                }
                KeyCode::Enter => app.searching = false,
                KeyCode::Backspace => {
                    app.search.pop();
                }
                KeyCode::Char(c) => app.search.push(c),
                _ => {}
            }
            continue;
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
            KeyCode::Tab | KeyCode::Right => {
                app.tab = (app.tab + 1) % TAB_KEYS.len();
                app.selected = 0;
            }
            KeyCode::Left => {
                app.tab = (app.tab + TAB_KEYS.len() - 1) % TAB_KEYS.len();
                app.selected = 0;
            }
            KeyCode::Char(c @ '1'..='5') => {
                app.tab = c as usize - '1' as usize;
                app.selected = 0;
            }
            KeyCode::Down | KeyCode::Char('j') => app.move_down(),
            KeyCode::Up | KeyCode::Char('k') => app.move_up(),
            KeyCode::Char('/') if app.tab == 0 => {
                app.searching = true;
                app.search.clear();
            }
            KeyCode::Char('c') if app.tab == 0 => {
                let items = app.filtered_vars();
                if let Some((_, v)) = items.get(app.selected) {
                    let v = v.clone();
                    app.copy(&v);
                }
            }
            KeyCode::Char('C') => {
                let json = app.json.clone();
                app.copy(&json);
            }
            KeyCode::Enter => app.on_enter(),
            _ => {}
        }

        // Side-effect hook run requested: leave the terminal temporarily to show command output directly.
        if app.pending_run_hooks {
            app.pending_run_hooks = false;
            run_hooks_suspended(terminal, app)?;
        }
    }
}

/// Temporarily restore the terminal, run exec side-effect hooks, then re-enter TUI mode.
fn run_hooks_suspended<B: Backend>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()>
where
    B::Error: std::error::Error + Send + Sync + 'static,
{
    if app.config.exec.is_empty() {
        app.status = t!("tui.status.no_exec_hooks").to_string();
        return Ok(());
    }
    // Return to the normal screen.
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
    println!("\n=== {} ===", t!("tui.exec_run_header"));
    let result = exec::run_hooks(&app.config.exec, None, &app.vars, &app.work_dir, false);
    app.status = match &result {
        Ok(_) => t!("tui.status.exec_done").to_string(),
        Err(e) => t!("tui.status.exec_failed", error = format!("{e}")).to_string(),
    };
    if let Err(e) = &result {
        println!("{}", t!("error.generic", error = format!("{e}")));
    }
    println!("\n{}", t!("tui.press_enter_return"));
    let mut line = String::new();
    let _ = io::stdin().read_line(&mut line);
    // Re-enter TUI mode.
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
    terminal.clear()?;
    Ok(())
}

impl App {
    fn list_len(&self) -> usize {
        match self.tab {
            0 => self.filtered_vars().len(),
            1 => EDITABLE_CONFIG.len(),
            2 => self.commits.len(),
            3 => self.branches.len(),
            4 => self.actions.len(),
            _ => 0,
        }
    }
    fn move_down(&mut self) {
        let len = self.list_len();
        if len > 0 {
            self.selected = (self.selected + 1).min(len - 1);
        }
    }
    fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }
    fn on_enter(&mut self) {
        match self.tab {
            1 => {
                // Edit the selected global config key, pre-filling the current value.
                if let Some((key, _)) = EDITABLE_CONFIG.get(self.selected) {
                    self.edit_config_key = Some((*key).to_string());
                    self.input_buf = global_value(&self.config, key);
                    self.input = Some(InputAction::EditConfig);
                }
            }
            3 => {
                if let Some(b) = self.branches.get(self.selected).cloned() {
                    self.branch_override = if b == self.base_branch {
                        None
                    } else {
                        Some(b.clone())
                    };
                    self.recompute();
                }
            }
            4 => self.run_action(self.selected),
            _ => {}
        }
    }

    /// Input modal prompt text (EditConfig shows the key name).
    fn input_prompt(&self) -> String {
        match (&self.input, &self.edit_config_key) {
            (Some(InputAction::EditConfig), Some(key)) => {
                let hint_key = EDITABLE_CONFIG
                    .iter()
                    .find(|(k, _)| k == key)
                    .map(|(_, h)| *h)
                    .unwrap_or("");
                t!("tui.config_edit_prompt", key = key, hint = t!(hint_key)).to_string()
            }
            (Some(a), _) => t!(a.prompt()).to_string(),
            _ => String::new(),
        }
    }
}

fn ui(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(f.area());

    // Header.
    let target = app.branch_override.as_deref().unwrap_or(&app.base_branch);
    let mut header_spans = vec![
        Span::styled(
            " GitVersion ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            &app.vars.full_sem_ver,
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!("   {}: ", t!("tui.header.branch"))),
        Span::styled(target, Style::default().fg(Color::Yellow)),
    ];
    if app.next_version_override.is_some() {
        header_spans.push(Span::styled(
            "  [next-version override]",
            Style::default().fg(Color::Magenta),
        ));
    }
    f.render_widget(
        Paragraph::new(Line::from(header_spans)).block(Block::default().borders(Borders::ALL)),
        chunks[0],
    );

    // Tabs.
    let tabs = Tabs::new(
        TAB_KEYS
            .iter()
            .enumerate()
            .map(|(i, k)| format!("{}:{}", i + 1, t!(*k)))
            .collect::<Vec<_>>(),
    )
    .select(app.tab)
    .block(Block::default().borders(Borders::ALL))
    .highlight_style(
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    );
    f.render_widget(tabs, chunks[1]);

    match app.tab {
        0 => render_variables(f, app, chunks[2]),
        1 => render_config(f, app, chunks[2]),
        2 => render_commits(f, app, chunks[2]),
        3 => render_branches(f, app, chunks[2]),
        _ => render_actions(f, app, chunks[2]),
    }

    // Footer (status + help).
    let help = match app.tab {
        0 => t!("tui.help.variables"),
        1 => t!("tui.help.config"),
        3 => t!("tui.help.branches"),
        4 => t!("tui.help.actions"),
        _ => t!("tui.help.default"),
    };
    let footer = Line::from(vec![
        Span::styled(
            format!(" {} ", app.status),
            Style::default().fg(Color::Black).bg(Color::Gray),
        ),
        Span::raw("  "),
        Span::styled(help.to_string(), Style::default().fg(Color::DarkGray)),
    ]);
    f.render_widget(Paragraph::new(footer), chunks[3]);

    // Input modal.
    if let Some(action) = &app.input {
        let _ = action;
        render_input_modal(f, &app.input_prompt(), &app.input_buf);
    }
}

fn render_variables(f: &mut Frame, app: &App, area: Rect) {
    let items = app.filtered_vars();
    let rows: Vec<Row> = items
        .iter()
        .enumerate()
        .map(|(i, (k, v))| {
            let style = if i == app.selected {
                Style::default().fg(Color::Black).bg(Color::Cyan)
            } else {
                Style::default().fg(Color::White)
            };
            Row::new(vec![
                Cell::from(k.clone()).style(Style::default().fg(Color::Cyan)),
                Cell::from(v.clone()),
            ])
            .style(style)
        })
        .collect();
    let title = if app.searching || !app.search.is_empty() {
        t!("tui.title.variables_search", query = app.search).to_string()
    } else {
        t!("tui.title.variables_count", count = items.len()).to_string()
    };
    let table = Table::new(
        rows,
        [Constraint::Percentage(38), Constraint::Percentage(62)],
    )
    .header(
        Row::new(vec![
            t!("tui.col.variable").to_string(),
            t!("tui.col.value").to_string(),
        ])
        .style(
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Yellow),
        ),
    )
    .block(Block::default().borders(Borders::ALL).title(title));
    f.render_widget(table, area);
}

fn render_config(f: &mut Frame, app: &App, area: Rect) {
    // Top: editable global config (selection list). Bottom: effective config result.
    let halves = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    let edit_items: Vec<ListItem> = EDITABLE_CONFIG
        .iter()
        .enumerate()
        .map(|(i, (key, _))| {
            let val = global_value(&app.config, key);
            let shown = if val.is_empty() {
                t!("tui.unset").to_string()
            } else {
                val
            };
            let style = if i == app.selected {
                Style::default().fg(Color::Black).bg(Color::Cyan)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(format!("{key:<28}{shown}")).style(style)
        })
        .collect();
    f.render_widget(
        List::new(edit_items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} ", t!("tui.title.global_config"))),
        ),
        halves[0],
    );

    let eff = EffectiveConfiguration::resolve(
        &app.config,
        app.branch_override.as_deref().unwrap_or(&app.base_branch),
    );
    let strategies: Vec<String> = if app.config.strategies.is_empty() {
        vec![t!("tui.default_paren").to_string()]
    } else {
        app.config
            .strategies
            .iter()
            .map(|s| format!("{s:?}"))
            .collect()
    };
    let none_paren = t!("tui.none_paren").to_string();
    let exec_hooks: String = if app.config.exec.is_empty() {
        none_paren.clone()
    } else {
        app.config
            .exec
            .keys()
            .cloned()
            .collect::<Vec<_>>()
            .join(", ")
    };
    let lines = vec![
        kv(&t!("tui.kv.matched_branch_key"), &eff.branch_key),
        kv("increment", &format!("{:?}", eff.increment)),
        kv("mode(deployment)", &format!("{:?}", eff.deployment_mode)),
        kv("label", &eff.label),
        kv("regex", eff.regex.as_deref().unwrap_or("")),
        kv("is-release-branch", &eff.is_release_branch.to_string()),
        kv("is-main-branch", &eff.is_main_branch.to_string()),
        kv(
            "tracks-release-branches",
            &eff.tracks_release_branches.to_string(),
        ),
        kv("track-merge-message", &eff.track_merge_message.to_string()),
        kv(
            "commit-message-incrementing",
            &format!("{:?}", eff.commit_message_incrementing),
        ),
        kv(
            "prevent-increment.of-merged",
            &eff.prevent_increment_of_merged_branch.to_string(),
        ),
        kv(
            "prevent-increment.when-tagged",
            &eff.prevent_increment_when_current_commit_tagged.to_string(),
        ),
        kv("pre-release-weight", &eff.pre_release_weight.to_string()),
        kv(
            "tag-pre-release-weight",
            &eff.tag_pre_release_weight.to_string(),
        ),
        kv("tag-prefix", &eff.tag_prefix),
        kv(
            "semantic-version-format",
            &format!("{:?}", eff.semantic_version_format),
        ),
        kv("source-branches", &eff.source_branches.join(", ")),
        kv("strategies", &strategies.join(", ")),
        kv(&t!("tui.kv.exec_hooks"), &exec_hooks),
        kv(
            "next-version",
            app.next_version_override
                .as_deref()
                .or(app.config.next_version.as_deref())
                .unwrap_or(&none_paren),
        ),
    ];
    let para = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", t!("tui.title.effective"))),
    );
    f.render_widget(para, halves[1]);
}

fn kv(k: &str, v: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{k:<30}"), Style::default().fg(Color::Cyan)),
        Span::styled(v.to_string(), Style::default().fg(Color::White)),
    ])
}

fn render_commits(f: &mut Frame, app: &App, area: Rect) {
    let src = app.vars.version_source_sha.clone();
    let items: Vec<ListItem> = app
        .commits
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let is_src = !src.is_empty() && c.sha.starts_with(&src[..src.len().min(c.sha.len())])
                || c.sha == src;
            let marker = if is_src { "◆ " } else { "  " };
            let date = c.when.format("%Y-%m-%d").to_string();
            let msg = c.message.lines().next().unwrap_or("");
            let line = format!("{marker}{} {date}  {}", &c.short_sha, truncate(msg, 60));
            let mut style = Style::default().fg(if is_src { Color::Green } else { Color::White });
            if i == app.selected {
                style = style.bg(Color::DarkGray);
            }
            ListItem::new(line).style(style)
        })
        .collect();
    let title = t!("tui.title.commits", count = app.commits.len()).to_string();
    f.render_widget(
        List::new(items).block(Block::default().borders(Borders::ALL).title(title)),
        area,
    );
}

fn render_branches(f: &mut Frame, app: &App, area: Rect) {
    let current = app.branch_override.as_deref().unwrap_or(&app.base_branch);
    let items: Vec<ListItem> = app
        .branches
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let mark = if b == current {
                "● "
            } else if b == &app.base_branch {
                "○ "
            } else {
                "  "
            };
            let mut style = Style::default().fg(Color::White);
            if i == app.selected {
                style = Style::default().fg(Color::Black).bg(Color::Cyan);
            } else if b == current {
                style = Style::default().fg(Color::Green);
            }
            ListItem::new(format!("{mark}{b}")).style(style)
        })
        .collect();
    f.render_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} ", t!("tui.title.branches"))),
        ),
        area,
    );
}

fn render_actions(f: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .actions
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let style = if i == app.selected {
                Style::default().fg(Color::Black).bg(Color::Cyan)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(format!("  {}", t!(*a))).style(style)
        })
        .collect();
    f.render_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} ", t!("tui.title.actions"))),
        ),
        area,
    );
}

fn render_input_modal(f: &mut Frame, prompt: &str, buf: &str) {
    let area = centered_rect(70, 20, f.area());
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} ", t!("tui.title.input_modal")))
        .border_style(Style::default().fg(Color::Magenta));
    let text = vec![
        Line::from(Span::styled(prompt, Style::default().fg(Color::Yellow))),
        Line::from(""),
        Line::from(Span::styled(
            format!("> {buf}_"),
            Style::default().fg(Color::White),
        )),
    ];
    f.render_widget(Paragraph::new(text).block(block), area);
}

fn centered_rect(px: u16, py: u16, r: Rect) -> Rect {
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - py) / 2),
            Constraint::Percentage(py),
            Constraint::Percentage((100 - py) / 2),
        ])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - px) / 2),
            Constraint::Percentage(px),
            Constraint::Percentage((100 - px) / 2),
        ])
        .split(v[1])[1]
}
