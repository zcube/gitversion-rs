//! Ratatui 기반 대화형 TUI.
//!
//! 계산된 버전 변수를 표 형태로 보여주고, 탭으로 JSON 원본을 전환한다.

use crate::output::{generator, VersionVariables};
use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, Tabs},
};
use std::io;
use std::time::Duration;

struct App {
    vars: VersionVariables,
    branch: String,
    json: String,
    tab: usize,
    scroll: u16,
}

/// TUI 실행. 계산 결과를 표시한다.
pub fn run(vars: VersionVariables, branch: String) -> Result<()> {
    let json = generator::to_json(&vars)?;
    let mut app = App { vars, branch, json, tab: 0, scroll: 0 };

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = event_loop(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;
    res
}

fn event_loop<B: Backend>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()> {
    loop {
        terminal.draw(|f| ui(f, app))?;
        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                    KeyCode::Tab | KeyCode::Right => app.tab = (app.tab + 1) % 2,
                    KeyCode::Left => app.tab = (app.tab + 1) % 2,
                    KeyCode::Char('1') => app.tab = 0,
                    KeyCode::Char('2') => app.tab = 1,
                    KeyCode::Down | KeyCode::Char('j') => app.scroll = app.scroll.saturating_add(1),
                    KeyCode::Up | KeyCode::Char('k') => app.scroll = app.scroll.saturating_sub(1),
                    _ => {}
                }
            }
        }
    }
}

fn ui(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Length(3), Constraint::Min(1), Constraint::Length(1)])
        .split(f.area());

    // 헤더: FullSemVer 강조.
    let header = Paragraph::new(Line::from(vec![
        Span::styled(" GitVersion ", Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw("  "),
        Span::styled(&app.vars.full_sem_ver, Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::raw("   브랜치: "),
        Span::styled(&app.branch, Style::default().fg(Color::Yellow)),
    ]))
    .block(Block::default().borders(Borders::ALL));
    f.render_widget(header, chunks[0]);

    // 탭.
    let tabs = Tabs::new(vec!["변수 표", "JSON"])
        .select(app.tab)
        .block(Block::default().borders(Borders::ALL))
        .highlight_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));
    f.render_widget(tabs, chunks[1]);

    match app.tab {
        0 => render_table(f, app, chunks[2]),
        _ => render_json(f, app, chunks[2]),
    }

    let help = Paragraph::new(Line::from(vec![Span::styled(
        " [Tab] 화면 전환  [↑/↓] 스크롤  [q] 종료 ",
        Style::default().fg(Color::DarkGray),
    )]));
    f.render_widget(help, chunks[3]);
}

fn render_table(f: &mut Frame, app: &App, area: Rect) {
    let rows: Vec<Row> = app
        .vars
        .to_map()
        .into_iter()
        .map(|(k, v)| {
            Row::new(vec![
                Cell::from(k).style(Style::default().fg(Color::Cyan)),
                Cell::from(v).style(Style::default().fg(Color::White)),
            ])
        })
        .collect();
    let table = Table::new(rows, [Constraint::Percentage(40), Constraint::Percentage(60)])
        .header(
            Row::new(vec!["변수", "값"])
                .style(Style::default().add_modifier(Modifier::BOLD).fg(Color::Yellow)),
        )
        .block(Block::default().borders(Borders::ALL).title(" 출력 변수 "));
    f.render_widget(table, area);
}

fn render_json(f: &mut Frame, app: &App, area: Rect) {
    let para = Paragraph::new(app.json.as_str())
        .block(Block::default().borders(Borders::ALL).title(" JSON "))
        .scroll((app.scroll, 0));
    f.render_widget(para, area);
}
