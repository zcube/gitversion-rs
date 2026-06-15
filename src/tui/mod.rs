//! Ratatui 기반 대화형 TUI.
//!
//! 단순 뷰어를 넘어, 계산 결과 탐색과 실제 버전에 영향을 주는 조작(태그/브랜치
//! 생성, next-version 설정, 캐시 삭제, 동적 clone, 브랜치별 재계산)을 제공한다.

use crate::config::effective::EffectiveConfiguration;
use crate::config::{loader, CommitMessageConvention, GitVersionConfiguration};
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
use std::io;
use std::path::PathBuf;
use std::time::Duration;

const TAB_TITLES: [&str; 5] = ["변수", "설정", "커밋", "브랜치", "액션"];

/// 텍스트 입력이 필요한 액션.
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
            InputAction::CreateTag => "HEAD 에 태그 생성 — 버전 입력(예: v1.2.0)",
            InputAction::CreateBranch => "HEAD 에 브랜치 생성 — 이름 입력",
            InputAction::SetNextVersion => "next-version 설정 — 버전 입력(예: 2.0.0)",
            InputAction::DynamicClone => "동적 clone — 'URL 브랜치' 입력",
            InputAction::EditExecHook => "exec 훅 편집 — '이름=명령' (이름: verify/prepare/publish/success/fail/version, 빈 명령은 삭제)",
            InputAction::EditConfig => "전역 설정 편집",
        }
    }
}

struct App {
    repo: GitRepo,
    config: GitVersionConfiguration,
    work_dir: PathBuf,
    /// 현재 체크아웃 브랜치(기준).
    base_branch: String,
    /// 재계산 대상 브랜치(브랜치 탭에서 선택). None 이면 base.
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
    /// 이벤트 루프가 터미널을 잠시 빠져나가 side-effect 훅을 실행하도록 요청.
    pending_run_hooks: bool,
    /// 편집 중인 전역 설정 키(EditConfig 입력용).
    edit_config_key: Option<String>,
    /// TUI 에서 변경한 전역 설정(key=value). 파일 저장 시 최소 diff 로 기록.
    tui_overrides: std::collections::BTreeMap<String, String>,
}

/// 설정 탭에서 편집 가능한 전역 설정 키(overrideconfig 와 동일 의미).
const EDITABLE_CONFIG: [(&str, &str); 14] = [
    ("increment", "None/Major/Minor/Patch/Inherit"),
    ("mode", "ContinuousDelivery/ContinuousDeployment/ManualDeployment"),
    ("label", "pre-release label"),
    ("tag-prefix", "예: [vV]?"),
    ("next-version", "예: 2.0.0"),
    ("commit-message-convention", "Default/ConventionalCommits"),
    ("semantic-version-format", "Strict/Loose"),
    ("tag-pre-release-weight", "정수"),
    ("update-build-number", "true/false"),
    ("commit-date-format", "예: yyyy-MM-dd"),
    ("major-version-bump-message", "정규식"),
    ("minor-version-bump-message", "정규식"),
    ("patch-version-bump-message", "정규식"),
    ("no-bump-message", "정규식"),
];

/// 문자열을 YAML 스칼라(bool/int/문자열)로 변환.
fn yaml_scalar(v: &str) -> serde_yaml::Value {
    if let Ok(b) = v.parse::<bool>() {
        return serde_yaml::Value::Bool(b);
    }
    if let Ok(i) = v.parse::<i64>() {
        return serde_yaml::Value::Number(i.into());
    }
    serde_yaml::Value::String(v.to_string())
}

/// 전역 설정 키의 현재 값을 문자열로.
fn global_value(config: &GitVersionConfiguration, key: &str) -> String {
    use crate::config::CommitMessageConvention as C;
    match key {
        "increment" => config.increment.map(|v| format!("{v:?}")).unwrap_or_default(),
        "mode" => config.mode.map(|v| format!("{v:?}")).unwrap_or_default(),
        "label" => config.label.clone().unwrap_or_default(),
        "tag-prefix" => config.tag_prefix.clone().unwrap_or_default(),
        "next-version" => config.next_version.clone().unwrap_or_default(),
        "commit-message-convention" => match config.commit_message_convention {
            Some(C::ConventionalCommits) => "ConventionalCommits".into(),
            _ => "Default".into(),
        },
        "semantic-version-format" => config.semantic_version_format.map(|v| format!("{v:?}")).unwrap_or_default(),
        "tag-pre-release-weight" => config.tag_pre_release_weight.map(|v| v.to_string()).unwrap_or_default(),
        "update-build-number" => config.update_build_number.map(|v| v.to_string()).unwrap_or_default(),
        "commit-date-format" => config.commit_date_format.clone().unwrap_or_default(),
        "major-version-bump-message" => config.major_version_bump_message.clone().unwrap_or_default(),
        "minor-version-bump-message" => config.minor_version_bump_message.clone().unwrap_or_default(),
        "patch-version-bump-message" => config.patch_version_bump_message.clone().unwrap_or_default(),
        "no-bump-message" => config.no_bump_message.clone().unwrap_or_default(),
        _ => String::new(),
    }
}

/// TUI 실행. 저장소·설정을 받아 대화형으로 동작한다.
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
        status: "준비 완료".into(),
        actions: vec![
            "태그 생성 (HEAD)",
            "브랜치 생성 (HEAD)",
            "next-version 설정",
            "Conventional Commits 토글",
            "exec 훅 편집",
            "exec 훅 실행 (prepare 등)",
            "설정 저장 (GitVersion.yml)",
            "캐시 삭제",
            "동적 원격 clone",
            "재계산",
            "기준 브랜치로 초기화",
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

    // 패닉이 나도 alternate screen 을 오염시키지 않도록 훅을 잠시 교체하고,
    // 패닉 메시지를 보관한다. catch_unwind 로 패닉을 잡아 우아하게 종료한다.
    let panic_msg: std::sync::Arc<std::sync::Mutex<Option<String>>> = Default::default();
    let captured = panic_msg.clone();
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let msg = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "알 수 없는 패닉".into());
        let loc = info.location().map(|l| format!(" ({}:{})", l.file(), l.line())).unwrap_or_default();
        *captured.lock().unwrap() = Some(format!("{msg}{loc}"));
    }));

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        event_loop(&mut terminal, &mut app)
    }));

    // 무슨 일이 있어도 터미널을 복구한다.
    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture);
    let _ = terminal.show_cursor();
    std::panic::set_hook(original_hook);

    match result {
        Ok(r) => r,
        Err(_) => {
            let msg = panic_msg.lock().unwrap().clone().unwrap_or_else(|| "내부 오류".into());
            // 패닉을 크래시가 아니라 일반 에러로 변환(터미널은 이미 복구됨).
            log::error!("TUI 내부 패닉을 방어했습니다: {msg}");
            Err(anyhow::anyhow!("TUI 가 내부 오류로 안전하게 종료되었습니다: {msg}"))
        }
    }
}

impl App {
    /// 현재 override 를 반영한 설정으로 재계산.
    fn recompute(&mut self) {
        let mut cfg = self.config.clone();
        if let Some(nv) = &self.next_version_override {
            cfg.next_version = Some(nv.clone());
        }
        match calculation::calculate(&self.repo, &cfg, self.branch_override.clone()) {
            Ok(mut v) => {
                // version exec 훅이 있으면 그 출력으로 버전을 수정해 재계산(CLI 와 동일).
                if let Some(cmd) = cfg.exec.get("version").cloned() {
                    if let Ok(Some(nv)) = exec::run_version_hook(&cmd, &v, &self.work_dir, false) {
                        cfg.next_version = Some(nv.clone());
                        if let Ok(v2) = calculation::calculate(&self.repo, &cfg, self.branch_override.clone()) {
                            v = v2;
                            self.status = format!("version 훅 적용: next-version={nv}");
                        }
                    }
                }
                self.json = generator::to_json(&v).unwrap_or_default();
                self.vars = v;
                if !self.status.starts_with("version 훅") {
                    self.status = format!(
                        "재계산 완료 ({})",
                        self.branch_override.as_deref().unwrap_or(&self.base_branch)
                    );
                }
            }
            Err(e) => self.status = format!("계산 오류: {e}"),
        }
        // 커밋 목록 갱신(대상 브랜치 기준).
        let target = self.branch_override.clone().unwrap_or_else(|| self.base_branch.clone());
        self.commits = self.repo.first_parent_between(None, &target).unwrap_or_default();
        self.commits.truncate(200);
    }

    fn reload_lists(&mut self) {
        self.branches = self.repo.local_branch_names().unwrap_or_default();
    }

    /// 현재 표시 중인 변수 (검색 필터 적용).
    fn filtered_vars(&self) -> Vec<(String, String)> {
        let q = self.search.to_lowercase();
        self.vars
            .to_map()
            .into_iter()
            .filter(|(k, v)| {
                q.is_empty()
                    || k.to_lowercase().contains(&q)
                    || v.to_lowercase().contains(&q)
            })
            .collect()
    }

    fn copy(&mut self, text: &str) {
        match arboard::Clipboard::new().and_then(|mut c| c.set_text(text.to_string())) {
            Ok(_) => self.status = format!("복사됨: {}", truncate(text, 40)),
            Err(e) => self.status = format!("클립보드 실패: {e}"),
        }
    }

    fn confirm_input(&mut self) {
        let action = self.input.take();
        let buf = std::mem::take(&mut self.input_buf);
        let buf = buf.trim().to_string();
        if buf.is_empty() {
            self.status = "입력 취소(빈 값)".into();
            return;
        }
        match action {
            Some(InputAction::CreateTag) => match self.repo.create_tag(&buf, None) {
                Ok(_) => {
                    self.status = format!("태그 생성: {buf}");
                    self.recompute();
                    self.reload_lists();
                }
                Err(e) => self.status = format!("태그 생성 실패: {e}"),
            },
            Some(InputAction::CreateBranch) => match self.repo.create_branch(&buf, None) {
                Ok(_) => {
                    self.status = format!("브랜치 생성: {buf}");
                    self.reload_lists();
                }
                Err(e) => self.status = format!("브랜치 생성 실패: {e}"),
            },
            Some(InputAction::SetNextVersion) => {
                self.next_version_override = Some(buf.clone());
                self.status = format!("next-version = {buf}");
                self.recompute();
            }
            Some(InputAction::DynamicClone) => self.do_dynamic_clone(&buf),
            Some(InputAction::EditExecHook) => {
                let Some((name, cmd)) = buf.split_once('=') else {
                    self.status = "형식: 이름=명령".into();
                    return;
                };
                let (name, cmd) = (name.trim().to_string(), cmd.trim().to_string());
                const VALID: [&str; 6] =
                    ["verify", "prepare", "publish", "success", "fail", "version"];
                if !VALID.contains(&name.as_str()) {
                    self.status = format!("알 수 없는 훅 이름: {name} (verify/prepare/publish/success/fail/version)");
                    return;
                }
                if cmd.is_empty() {
                    self.config.exec.remove(&name);
                    self.status = format!("exec 훅 삭제: {name}");
                } else {
                    self.config.exec.insert(name.clone(), cmd);
                    self.status = format!("exec 훅 설정: {name}");
                }
                // version 훅 변경은 버전에 영향 → 재계산 후 저장.
                self.recompute();
                self.save_config();
            }
            Some(InputAction::EditConfig) => {
                if let Some(key) = self.edit_config_key.take() {
                    self.apply_global_edit(&key, &buf);
                    self.status = format!("{key} = {buf} (저장됨)");
                }
            }
            None => {}
        }
    }

    fn do_dynamic_clone(&mut self, spec: &str) {
        let mut parts = spec.split_whitespace();
        let (url, branch) = (parts.next(), parts.next());
        let Some(url) = url else {
            self.status = "URL 이 필요합니다".into();
            return;
        };
        let opts = DynamicRepoOptions {
            url: url.to_string(),
            branch: branch.map(|s| s.to_string()).or_else(|| Some("main".into())),
            username: None,
            password: None,
            commit: None,
            location: None,
        };
        self.status = "clone 중...".into();
        match remote::prepare(&opts) {
            Ok(dest) => match GitRepo::discover(&dest) {
                Ok(repo) => {
                    let root = repo.workdir().map(|p| p.to_path_buf()).unwrap_or_else(|| dest.clone());
                    self.config =
                        loader::load(None, &root, Some(&root)).unwrap_or_else(|_| self.config.clone());
                    self.repo = repo;
                    self.work_dir = root;
                    self.base_branch = self.repo.current_branch_name().unwrap_or_default();
                    self.branch_override = None;
                    self.next_version_override = None;
                    self.recompute();
                    self.reload_lists();
                    self.status = format!("clone 완료: {url}");
                }
                Err(e) => self.status = format!("clone 저장소 열기 실패: {e}"),
            },
            Err(e) => self.status = format!("clone 실패: {e}"),
        }
    }

    fn run_action(&mut self, idx: usize) {
        match idx {
            0 => self.start_input(InputAction::CreateTag),
            1 => self.start_input(InputAction::CreateBranch),
            2 => self.start_input(InputAction::SetNextVersion),
            3 => {
                // Conventional Commits 토글 후 재계산.
                let cur = self
                    .config
                    .commit_message_convention
                    .unwrap_or(CommitMessageConvention::Default);
                let next = match cur {
                    CommitMessageConvention::ConventionalCommits => CommitMessageConvention::Default,
                    CommitMessageConvention::Default => CommitMessageConvention::ConventionalCommits,
                };
                let val = format!("{next:?}");
                self.config.commit_message_convention = Some(next);
                self.tui_overrides.insert("commit-message-convention".into(), val.clone());
                self.recompute();
                self.save_config();
                self.status = format!("commit-message-convention = {val} (저장됨)");
            }
            4 => self.start_input(InputAction::EditExecHook),
            5 => self.pending_run_hooks = true, // 이벤트 루프가 터미널을 빠져나가 실행.
            6 => self.save_config(),
            7 => match self.repo.clear_cache() {
                Ok(n) => self.status = format!("캐시 삭제: {n}개 파일"),
                Err(e) => self.status = format!("캐시 삭제 실패: {e}"),
            },
            8 => self.start_input(InputAction::DynamicClone),
            9 => self.recompute(),
            10 => {
                self.branch_override = None;
                self.next_version_override = None;
                self.recompute();
                self.status = format!("기준 브랜치({})로 초기화", self.base_branch);
            }
            _ => {}
        }
    }

    /// 변경된 전역 설정을 GitVersion.yml 에 최소 diff 로 저장(기존 파일 보존).
    fn save_config(&mut self) {
        let path = self.work_dir.join("GitVersion.yml");
        let mut doc: serde_yaml::Mapping = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_yaml::from_str(&s).ok())
            .unwrap_or_default();

        // 변경한 전역 키들을 스칼라로 기록.
        for (k, v) in &self.tui_overrides {
            doc.insert(serde_yaml::Value::String(k.clone()), yaml_scalar(v));
        }
        // exec 훅 맵.
        if self.config.exec.is_empty() {
            doc.remove(serde_yaml::Value::String("exec".into()));
        } else {
            let mut exec_map = serde_yaml::Mapping::new();
            for (k, v) in &self.config.exec {
                exec_map.insert(serde_yaml::Value::String(k.clone()), serde_yaml::Value::String(v.clone()));
            }
            doc.insert(serde_yaml::Value::String("exec".into()), serde_yaml::Value::Mapping(exec_map));
        }

        match serde_yaml::to_string(&doc).map_err(anyhow::Error::from).and_then(|y| {
            std::fs::write(&path, y).map_err(anyhow::Error::from)
        }) {
            Ok(_) => self.status = format!("설정 저장: {}", path.display()),
            Err(e) => self.status = format!("설정 저장 실패: {e}"),
        }
    }

    /// 전역 설정 키를 편집(overrideconfig 동일 로직) + 기록 + 재계산 + 저장.
    fn apply_global_edit(&mut self, key: &str, value: &str) {
        crate::cli::apply_overrides(&mut self.config, &[format!("{key}={value}")]);
        self.tui_overrides.insert(key.to_string(), value.to_string());
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

fn event_loop<B: Backend>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()> {
    loop {
        terminal.draw(|f| ui(f, app))?;
        if !event::poll(Duration::from_millis(200))? {
            continue;
        }
        let Event::Key(key) = event::read()? else { continue };
        if key.kind != KeyEventKind::Press {
            continue;
        }

        // 입력 모달 우선.
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

        // 검색 입력 모드(변수 탭).
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
                app.tab = (app.tab + 1) % TAB_TITLES.len();
                app.selected = 0;
            }
            KeyCode::Left => {
                app.tab = (app.tab + TAB_TITLES.len() - 1) % TAB_TITLES.len();
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

        // side-effect 훅 실행 요청: 터미널을 잠시 빠져나가 명령 출력을 그대로 보여준다.
        if app.pending_run_hooks {
            app.pending_run_hooks = false;
            run_hooks_suspended(terminal, app)?;
        }
    }
}

/// 터미널을 일시 복구해 exec side-effect 훅을 실행하고 다시 진입한다.
fn run_hooks_suspended<B: Backend>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()> {
    if app.config.exec.is_empty() {
        app.status = "설정된 exec 훅이 없습니다(액션 'exec 훅 편집'으로 추가)".into();
        return Ok(());
    }
    // 일반 화면으로 복귀.
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
    println!("\n=== exec 훅 실행 ===");
    let result = exec::run_hooks(&app.config.exec, None, &app.vars, &app.work_dir, false);
    app.status = match &result {
        Ok(_) => "exec 훅 실행 완료".into(),
        Err(e) => format!("exec 훅 실패: {e}"),
    };
    if let Err(e) = &result {
        println!("오류: {e}");
    }
    println!("\n[Enter] 를 누르면 TUI 로 돌아갑니다...");
    let mut line = String::new();
    let _ = io::stdin().read_line(&mut line);
    // TUI 재진입.
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
                // 선택한 전역 설정 키 편집(현재 값 미리 채움).
                if let Some((key, _)) = EDITABLE_CONFIG.get(self.selected) {
                    self.edit_config_key = Some((*key).to_string());
                    self.input_buf = global_value(&self.config, key);
                    self.input = Some(InputAction::EditConfig);
                }
            }
            3 => {
                if let Some(b) = self.branches.get(self.selected).cloned() {
                    self.branch_override =
                        if b == self.base_branch { None } else { Some(b.clone()) };
                    self.recompute();
                }
            }
            4 => self.run_action(self.selected),
            _ => {}
        }
    }

    /// 입력 모달 프롬프트(EditConfig 는 키 표시).
    fn input_prompt(&self) -> String {
        match (&self.input, &self.edit_config_key) {
            (Some(InputAction::EditConfig), Some(key)) => {
                let hint = EDITABLE_CONFIG.iter().find(|(k, _)| k == key).map(|(_, h)| *h).unwrap_or("");
                format!("{key} 설정 — {hint}")
            }
            (Some(a), _) => a.prompt().to_string(),
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

    // 헤더.
    let target = app.branch_override.as_deref().unwrap_or(&app.base_branch);
    let mut header_spans = vec![
        Span::styled(" GitVersion ", Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw("  "),
        Span::styled(&app.vars.full_sem_ver, Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::raw("   브랜치: "),
        Span::styled(target, Style::default().fg(Color::Yellow)),
    ];
    if app.next_version_override.is_some() {
        header_spans.push(Span::styled("  [next-version override]", Style::default().fg(Color::Magenta)));
    }
    f.render_widget(Paragraph::new(Line::from(header_spans)).block(Block::default().borders(Borders::ALL)), chunks[0]);

    // 탭.
    let tabs = Tabs::new(TAB_TITLES.iter().enumerate().map(|(i, t)| format!("{}:{t}", i + 1)).collect::<Vec<_>>())
        .select(app.tab)
        .block(Block::default().borders(Borders::ALL))
        .highlight_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));
    f.render_widget(tabs, chunks[1]);

    match app.tab {
        0 => render_variables(f, app, chunks[2]),
        1 => render_config(f, app, chunks[2]),
        2 => render_commits(f, app, chunks[2]),
        3 => render_branches(f, app, chunks[2]),
        _ => render_actions(f, app, chunks[2]),
    }

    // 푸터(상태 + 도움말).
    let help = match app.tab {
        0 => "[/]검색 [c]값복사 [C]JSON복사 [↑↓]이동 [1-5]탭 [q]종료",
        1 => "[Enter]설정 편집(저장됨) [↑↓]이동 [1-5]탭 [q]종료",
        3 => "[Enter]해당 브랜치로 재계산 [↑↓]이동 [q]종료",
        4 => "[Enter]실행 [↑↓]이동 [q]종료",
        _ => "[↑↓]스크롤/이동 [1-5]탭 [C]JSON복사 [q]종료",
    };
    let footer = Line::from(vec![
        Span::styled(format!(" {} ", app.status), Style::default().fg(Color::Black).bg(Color::Gray)),
        Span::raw("  "),
        Span::styled(help, Style::default().fg(Color::DarkGray)),
    ]);
    f.render_widget(Paragraph::new(footer), chunks[3]);

    // 입력 모달.
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
            Row::new(vec![Cell::from(k.clone()).style(Style::default().fg(Color::Cyan)), Cell::from(v.clone())]).style(style)
        })
        .collect();
    let title = if app.searching || !app.search.is_empty() {
        format!(" 변수  검색: {}_ ", app.search)
    } else {
        format!(" 변수 ({}개) ", items.len())
    };
    let table = Table::new(rows, [Constraint::Percentage(38), Constraint::Percentage(62)])
        .header(Row::new(vec!["변수", "값"]).style(Style::default().add_modifier(Modifier::BOLD).fg(Color::Yellow)))
        .block(Block::default().borders(Borders::ALL).title(title));
    f.render_widget(table, area);
}

fn render_config(f: &mut Frame, app: &App, area: Rect) {
    // 위: 편집 가능한 전역 설정(선택 목록), 아래: effective 결과.
    let halves = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    let edit_items: Vec<ListItem> = EDITABLE_CONFIG
        .iter()
        .enumerate()
        .map(|(i, (key, _))| {
            let val = global_value(&app.config, key);
            let shown = if val.is_empty() { "(미설정)".to_string() } else { val };
            let style = if i == app.selected {
                Style::default().fg(Color::Black).bg(Color::Cyan)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(format!("{key:<28}{shown}")).style(style)
        })
        .collect();
    f.render_widget(
        List::new(edit_items)
            .block(Block::default().borders(Borders::ALL).title(" 전역 설정 (Enter=편집, 변경 시 GitVersion.yml 저장) ")),
        halves[0],
    );

    let eff = EffectiveConfiguration::resolve(&app.config, app.branch_override.as_deref().unwrap_or(&app.base_branch));
    let strategies: Vec<String> = if app.config.strategies.is_empty() {
        vec!["(기본)".into()]
    } else {
        app.config.strategies.iter().map(|s| format!("{s:?}")).collect()
    };
    let exec_hooks: String = if app.config.exec.is_empty() {
        "(없음)".into()
    } else {
        app.config.exec.keys().cloned().collect::<Vec<_>>().join(", ")
    };
    let lines = vec![
        kv("매칭 브랜치 키", &eff.branch_key),
        kv("increment", &format!("{:?}", eff.increment)),
        kv("mode(deployment)", &format!("{:?}", eff.deployment_mode)),
        kv("label", &eff.label),
        kv("regex", eff.regex.as_deref().unwrap_or("")),
        kv("is-release-branch", &eff.is_release_branch.to_string()),
        kv("is-main-branch", &eff.is_main_branch.to_string()),
        kv("tracks-release-branches", &eff.tracks_release_branches.to_string()),
        kv("track-merge-message", &eff.track_merge_message.to_string()),
        kv("commit-message-incrementing", &format!("{:?}", eff.commit_message_incrementing)),
        kv("commit-message-convention", &format!("{:?}", eff.commit_message_convention)),
        kv("prevent-increment.of-merged", &eff.prevent_increment_of_merged_branch.to_string()),
        kv("prevent-increment.when-tagged", &eff.prevent_increment_when_current_commit_tagged.to_string()),
        kv("pre-release-weight", &eff.pre_release_weight.to_string()),
        kv("tag-pre-release-weight", &eff.tag_pre_release_weight.to_string()),
        kv("tag-prefix", &eff.tag_prefix),
        kv("semantic-version-format", &format!("{:?}", eff.semantic_version_format)),
        kv("source-branches", &eff.source_branches.join(", ")),
        kv("strategies", &strategies.join(", ")),
        kv("exec 훅", &exec_hooks),
        kv("next-version", app.next_version_override.as_deref().or(app.config.next_version.as_deref()).unwrap_or("(없음)")),
    ];
    let para = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(" 유효 설정(effective) — 위 설정이 이 브랜치에 해석된 결과 "));
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
            let is_src = !src.is_empty() && c.sha.starts_with(&src[..src.len().min(c.sha.len())]) || c.sha == src;
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
    let title = format!(" 커밋 (first-parent, {}개)  ◆=버전 소스 ", app.commits.len());
    f.render_widget(List::new(items).block(Block::default().borders(Borders::ALL).title(title)), area);
}

fn render_branches(f: &mut Frame, app: &App, area: Rect) {
    let current = app.branch_override.as_deref().unwrap_or(&app.base_branch);
    let items: Vec<ListItem> = app
        .branches
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let mark = if b == current { "● " } else if b == &app.base_branch { "○ " } else { "  " };
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
        List::new(items).block(Block::default().borders(Borders::ALL).title(" 브랜치 (Enter=재계산, ●현재 ○기준) ")),
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
            ListItem::new(format!("  {a}")).style(style)
        })
        .collect();
    f.render_widget(
        List::new(items).block(Block::default().borders(Borders::ALL).title(" 액션 (Enter=실행) ")),
        area,
    );
}

fn render_input_modal(f: &mut Frame, prompt: &str, buf: &str) {
    let area = centered_rect(70, 20, f.area());
    f.render_widget(Clear, area);
    let block = Block::default().borders(Borders::ALL).title(" 입력 (Enter 확인, Esc 취소) ").border_style(Style::default().fg(Color::Magenta));
    let text = vec![
        Line::from(Span::styled(prompt, Style::default().fg(Color::Yellow))),
        Line::from(""),
        Line::from(Span::styled(format!("> {buf}_"), Style::default().fg(Color::White))),
    ];
    f.render_widget(Paragraph::new(text).block(block), area);
}

fn centered_rect(px: u16, py: u16, r: Rect) -> Rect {
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage((100 - py) / 2), Constraint::Percentage(py), Constraint::Percentage((100 - py) / 2)])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage((100 - px) / 2), Constraint::Percentage(px), Constraint::Percentage((100 - px) / 2)])
        .split(v[1])[1]
}
