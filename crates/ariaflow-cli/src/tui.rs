//! `ariaflow --tui` — terminal UI for AriaFlow.
//!
//! Connects to the running `ariaflowd` over the same transport as the CLI
//! (pipe by default, web API with `--transport webapi`). Closing the TUI
//! never stops downloads; the daemon keeps running independently.

use ariaflow_api::client::AriaFlowClient;
use ariaflow_core::{
    Download, DownloadCounts, DownloadFilter, DownloadId, DownloadStatus, NewDownloadRequest,
    SortField, SortOrder,
};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use std::io::{stdout, Stdout};
use std::time::{Duration, Instant};

/// Sort orders cycled with `s`. `None` = daemon default (created desc).
const SORTS: &[(Option<SortField>, SortOrder, &str)] = &[
    (None, SortOrder::Desc, "created"),
    (Some(SortField::Filename), SortOrder::Asc, "name"),
    (Some(SortField::Size), SortOrder::Desc, "size"),
    (Some(SortField::Speed), SortOrder::Desc, "speed"),
    (Some(SortField::Progress), SortOrder::Desc, "progress"),
    (Some(SortField::Priority), SortOrder::Desc, "priority"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewFilter {
    All,
    Active, // downloading + queued + retrying
    Completed,
    Failed,
}

impl ViewFilter {
    fn title(&self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Active => "Active",
            Self::Completed => "Completed",
            Self::Failed => "Failed",
        }
    }

    fn next(&self) -> Self {
        match self {
            Self::All => Self::Active,
            Self::Active => Self::Completed,
            Self::Completed => Self::Failed,
            Self::Failed => Self::All,
        }
    }

    fn statuses(&self) -> Vec<DownloadStatus> {
        match self {
            Self::All => vec![],
            Self::Active => vec![
                DownloadStatus::Downloading,
                DownloadStatus::Connecting,
                DownloadStatus::Queued,
                DownloadStatus::Pending,
                DownloadStatus::Retrying,
                DownloadStatus::Waiting,
                DownloadStatus::Paused,
                DownloadStatus::Merging,
                DownloadStatus::Verifying,
            ],
            Self::Completed => vec![DownloadStatus::Completed],
            Self::Failed => vec![DownloadStatus::Failed, DownloadStatus::Cancelled],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum InputMode {
    Normal,
    /// Text being typed in the "add URL" prompt.
    Adding(String),
    /// Confirmation prompt for deleting a task id.
    ConfirmDelete(DownloadId),
    /// Text being typed in the "search filter" prompt.
    Search(String),
    /// Confirmation prompt for clearing all completed tasks.
    ConfirmClear,
}

struct App {
    client: AriaFlowClient,
    filter: ViewFilter,
    downloads: Vec<Download>,
    list_state: ListState,
    mode: InputMode,
    status_line: String,
    daemon_ok: bool,
    connected_since: Option<Instant>,
    global_speed: u64,
    counts: Option<DownloadCounts>,
    /// Active search filter (None / empty = no filter); applied server-side.
    search: Option<String>,
    /// Index into SORTS.
    sort_idx: usize,
    /// `?` toggles the key-bindings overlay.
    show_help: bool,
}

impl App {
    fn new(client: AriaFlowClient) -> Self {
        Self {
            client,
            filter: ViewFilter::All,
            downloads: Vec::new(),
            list_state: ListState::default(),
            mode: InputMode::Normal,
            status_line: "connecting to daemon...".to_string(),
            daemon_ok: false,
            connected_since: None,
            global_speed: 0,
            counts: None,
            search: None,
            sort_idx: 0,
            show_help: false,
        }
    }

    async fn refresh(&mut self) {
        let (sort_by, sort_order, _) = SORTS[self.sort_idx];
        let filter = DownloadFilter {
            statuses: self.filter.statuses(),
            search: self.search.clone().filter(|s| !s.trim().is_empty()),
            sort_by,
            sort_order,
            ..DownloadFilter::unfiltered()
        };
        match self.client.list(&filter).await {
            Ok(items) => {
                let selected = self.list_state.selected();
                self.downloads = items;
                self.daemon_ok = true;
                if self.connected_since.is_none() {
                    self.connected_since = Some(Instant::now());
                }
                // Keep selection in range
                if self.downloads.is_empty() {
                    self.list_state.select(None);
                } else {
                    let idx = selected.unwrap_or(0).min(self.downloads.len() - 1);
                    self.list_state.select(Some(idx));
                }
                // Also refresh aggregate counters for the header
                if let Ok(status) = self.client.status().await {
                    self.global_speed = status.counts.global_speed_bytes_per_sec;
                    self.counts = Some(status.counts);
                }
                let sort_hint = SORTS[self.sort_idx].2;
                self.status_line = match &self.search {
                    Some(q) if !q.trim().is_empty() => {
                        format!("{} task(s) · sort {sort_hint} · /{q}", self.downloads.len())
                    }
                    _ => format!("{} task(s) · sort {sort_hint} · live", self.downloads.len()),
                };
            }
            Err(e) => {
                self.daemon_ok = false;
                self.connected_since = None;
                self.status_line = format!("daemon unreachable: {e}");
            }
        }
    }

    async fn retry_selected(&mut self) {
        let Some(dl) = self.selected_download().cloned() else {
            return;
        };
        match dl.status {
            DownloadStatus::Failed | DownloadStatus::Cancelled => {
                match self.client.retry(dl.id).await {
                    Ok(()) => self.status_line = format!("retrying {}", dl.filename),
                    Err(e) => self.status_line = format!("error: {e}"),
                }
            }
            other => {
                self.status_line =
                    format!("can only retry failed/cancelled tasks (this one is {other})")
            }
        }
        self.refresh().await;
    }

    async fn bump_priority(&mut self, delta: i32) {
        let Some(dl) = self.selected_download().cloned() else {
            return;
        };
        let next = (dl.priority + delta).clamp(-100, 100);
        match self.client.set_priority(dl.id, next).await {
            Ok(()) => self.status_line = format!("priority of {} → {next}", dl.filename),
            Err(e) => self.status_line = format!("error: {e}"),
        }
        self.refresh().await;
    }

    async fn clear_completed(&mut self) {
        let completed: Vec<Download> = self
            .downloads
            .iter()
            .filter(|d| d.status == DownloadStatus::Completed)
            .cloned()
            .collect();
        if completed.is_empty() {
            self.status_line = "nothing to clear (no completed tasks in this view)".to_string();
            return;
        }
        let total = completed.len();
        let mut removed = 0usize;
        for dl in completed {
            if self.client.remove(dl.id, false).await.is_ok() {
                removed += 1;
            }
        }
        self.status_line = format!("removed {removed}/{total} completed task(s)");
        self.refresh().await;
    }

    fn cycle_sort(&mut self) {
        self.sort_idx = (self.sort_idx + 1) % SORTS.len();
        let sort_name = SORTS[self.sort_idx].2;
        self.status_line = format!("sort: {sort_name}");
    }

    fn jump_selection(&mut self, to_end: bool) {
        if self.downloads.is_empty() {
            self.list_state.select(None);
            return;
        }
        self.list_state
            .select(Some(if to_end { self.downloads.len() - 1 } else { 0 }));
    }

    fn selected_download(&self) -> Option<&Download> {
        self.list_state
            .selected()
            .and_then(|i| self.downloads.get(i))
    }

    fn move_selection(&mut self, delta: isize) {
        if self.downloads.is_empty() {
            self.list_state.select(None);
            return;
        }
        let len = self.downloads.len() as isize;
        let cur = self.list_state.selected().unwrap_or(0) as isize;
        let next = (cur + delta).rem_euclid(len) as usize;
        self.list_state.select(Some(next));
    }

    async fn toggle_pause_resume(&mut self) {
        let Some(dl) = self.selected_download().cloned() else {
            return;
        };
        let result = match dl.status {
            DownloadStatus::Paused => self.client.resume(dl.id).await,
            _ if dl.status.is_pausable() => self.client.pause(dl.id).await,
            _ => {
                self.status_line = format!("cannot toggle a {} task", dl.status);
                return;
            }
        };
        match result {
            Ok(()) => self.status_line = format!("toggled {}", dl.id),
            Err(e) => self.status_line = format!("error: {e}"),
        }
        self.refresh().await;
    }

    async fn delete_selected(&mut self) {
        if let Some(dl) = self.selected_download() {
            self.mode = InputMode::ConfirmDelete(dl.id);
        }
    }

    async fn confirm_delete(&mut self) {
        if let InputMode::ConfirmDelete(id) = std::mem::replace(&mut self.mode, InputMode::Normal) {
            match self.client.remove(id, false).await {
                Ok(()) => self.status_line = format!("removed {id}"),
                Err(e) => self.status_line = format!("error: {e}"),
            }
            self.refresh().await;
        }
    }

    async fn submit_add(&mut self, url: String) {
        let url = url.trim().to_string();
        if url.is_empty() {
            self.status_line = "empty URL, nothing added".to_string();
            return;
        }
        let req = NewDownloadRequest::new(url);
        match self.client.add(&req).await {
            Ok(dl) => self.status_line = format!("added {} ({})", dl.filename, dl.id),
            Err(e) => self.status_line = format!("error adding download: {e}"),
        }
        self.refresh().await;
    }
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}

fn human_eta(secs: Option<u64>) -> String {
    match secs {
        None => "--".into(),
        Some(s) if s < 60 => format!("{s}s"),
        Some(s) if s < 3600 => format!("{}m{:02}s", s / 60, s % 60),
        Some(s) => format!("{}h{:02}m", s / 3600, (s % 3600) / 60),
    }
}

fn status_color(status: DownloadStatus) -> Color {
    match status {
        DownloadStatus::Downloading => Color::Cyan,
        DownloadStatus::Completed => Color::Green,
        DownloadStatus::Failed => Color::Red,
        DownloadStatus::Paused => Color::Yellow,
        DownloadStatus::Queued
        | DownloadStatus::Pending
        | DownloadStatus::Waiting
        | DownloadStatus::Retrying => Color::DarkGray,
        DownloadStatus::Cancelled => Color::Magenta,
        DownloadStatus::Connecting | DownloadStatus::Merging | DownloadStatus::Verifying => {
            Color::Blue
        }
    }
}

fn ui(frame: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // header
            Constraint::Min(8),    // main
            Constraint::Length(3), // input / status
            Constraint::Length(1), // footer
        ])
        .split(frame.area());

    // --- header ---
    let header = Line::from(vec![
        Span::styled(
            " AriaFlow ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!("  {}  ", app.filter.title())),
        Span::styled(
            if app.daemon_ok {
                "● daemon online"
            } else {
                "○ daemon offline"
            },
            Style::default().fg(if app.daemon_ok {
                Color::Green
            } else {
                Color::Red
            }),
        ),
        Span::raw(format!("   ↓ {}/s", human_bytes(app.global_speed))),
        match &app.counts {
            Some(c) => Span::styled(
                format!(
                    "   act {} · wait {} · done {} · fail {}",
                    c.downloading, c.waiting, c.completed, c.failed
                ),
                Style::default().fg(Color::DarkGray),
            ),
            None => Span::raw(""),
        },
    ]);
    frame.render_widget(Paragraph::new(header), chunks[0]);

    // --- main split ---
    let main = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(chunks[1]);

    // list
    let items: Vec<ListItem> = app
        .downloads
        .iter()
        .map(|d| {
            let pct = d.progress.clamp(0.0, 100.0);
            let line = Line::from(vec![
                Span::styled(
                    format!("{:<13} ", d.status),
                    Style::default().fg(status_color(d.status)),
                ),
                Span::raw(format!("{:<32} ", truncate(&d.filename, 32))),
                Span::raw(format!(
                    "{pct:>5.1}%  {}",
                    d.total_size.map(human_bytes).unwrap_or_else(|| "?".into())
                )),
            ]);
            ListItem::new(line)
        })
        .collect();
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(format!(
                    " Downloads — {} · {}{} ",
                    app.filter.title(),
                    SORTS[app.sort_idx].2,
                    app.search
                        .as_deref()
                        .filter(|s| !s.trim().is_empty())
                        .map(|q| format!(" · /{q}"))
                        .unwrap_or_default()
                )))
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(40, 44, 52))
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");
    frame.render_stateful_widget(list, main[0], &mut app.list_state);

    // detail panel
    let detail_block = Block::default().borders(Borders::ALL).title(" Details ");
    let detail_inner = detail_block.inner(main[1]);
    frame.render_widget(detail_block, main[1]);
    let detail_text = match app.selected_download() {
        Some(d) => {
            let mut lines = vec![
                Line::from(Span::styled(
                    d.filename.clone(),
                    Style::default().add_modifier(Modifier::BOLD),
                )),
                Line::from(format!("url:      {}", d.url)),
                Line::from(format!("path:     {}", d.save_path.display())),
                Line::from(format!("status:   {}", d.status)),
                Line::from(format!("progress: {:.1}%", d.progress)),
                Line::from(format!(
                    "size:     {}/{}",
                    human_bytes(d.downloaded_size),
                    d.total_size.map(human_bytes).unwrap_or_else(|| "?".into())
                )),
                Line::from(format!(
                    "speed:    {}/s (avg {})",
                    human_bytes(d.speed_bytes_per_sec),
                    human_bytes(d.average_speed_bytes_per_sec)
                )),
                Line::from(format!("eta:      {}", human_eta(d.eta_seconds()))),
                Line::from(format!("category: {}", d.category)),
                Line::from(format!(
                    "priority: {}  conns: {}/{}",
                    d.priority, d.current_connections, d.max_connections
                )),
                Line::from(format!("id:       {}", d.id)),
            ];
            if let Some(err) = &d.error_message {
                lines.push(Line::from(Span::styled(
                    format!("error: {err}"),
                    Style::default().fg(Color::Red),
                )));
            }
            lines
        }
        None => vec![Line::from("no task selected")],
    };
    frame.render_widget(
        Paragraph::new(detail_text).wrap(Wrap { trim: true }),
        detail_inner,
    );

    // --- input/status line ---
    let status_block = Block::default()
        .borders(Borders::ALL)
        .title(match &app.mode {
            InputMode::Adding(_) => " Add download ",
            InputMode::ConfirmDelete(_) => " Confirm ",
            InputMode::Search(_) => " Search ",
            InputMode::ConfirmClear => " Clear completed ",
            InputMode::Normal => " Status ",
        });
    let status_inner = status_block.inner(chunks[2]);
    frame.render_widget(status_block, chunks[2]);
    let status_paragraph = match &app.mode {
        InputMode::Adding(buffer) => Paragraph::new(Line::from(vec![
            Span::raw("URL: "),
            Span::styled(buffer.clone(), Style::default().fg(Color::Cyan)),
            Span::raw("▏"),
        ])),
        InputMode::ConfirmDelete(id) => Paragraph::new(Line::from(vec![
            Span::raw(format!("Delete task {id}? ")),
            Span::styled("[y] yes  [n/Esc] no", Style::default().fg(Color::Yellow)),
        ])),
        InputMode::Search(buffer) => Paragraph::new(Line::from(vec![
            Span::raw("Filter: "),
            Span::styled(buffer.clone(), Style::default().fg(Color::Cyan)),
            Span::raw("▏"),
            Span::styled(
                "  [Enter] apply  [Esc] clear",
                Style::default().fg(Color::DarkGray),
            ),
        ])),
        InputMode::ConfirmClear => Paragraph::new(Line::from(vec![
            Span::raw("Remove all completed tasks in this view? "),
            Span::styled("[y] yes  [n/Esc] no", Style::default().fg(Color::Yellow)),
        ])),
        InputMode::Normal => Paragraph::new(app.status_line.clone()),
    };
    frame.render_widget(status_paragraph, status_inner);

    // --- footer key hints ---
    let key = |s: &'static str| Span::styled(s, Style::default().add_modifier(Modifier::BOLD));
    let footer = Paragraph::new(Line::from(vec![
        key("q"),
        Span::raw(" quit  "),
        key("↑↓/jk"),
        Span::raw(" move  "),
        key("Tab"),
        Span::raw(" view  "),
        key("/"),
        Span::raw(" search  "),
        key("s"),
        Span::raw(" sort  "),
        key("a"),
        Span::raw(" add  "),
        key("p/r"),
        Span::raw(" pause/resume  "),
        key("t"),
        Span::raw(" retry  "),
        key("+/-"),
        Span::raw(" priority  "),
        key("d"),
        Span::raw(" del  "),
        key("c"),
        Span::raw(" clear done  "),
        key("?"),
        Span::raw(" help"),
    ]))
    .alignment(Alignment::Center);
    frame.render_widget(footer, chunks[3]);

    if matches!(
        app.mode,
        InputMode::ConfirmDelete(_) | InputMode::ConfirmClear
    ) {
        frame.render_widget(Clear, chunks[2]);
    }

    // --- key-bindings overlay ---
    if app.show_help {
        let area = centered_rect(62, 70, frame.area());
        frame.render_widget(Clear, area);
        let lines = vec![
            Line::from(Span::styled(
                " Keys ",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("q / Ctrl-C      quit"),
            Line::from("↑↓ · j/k        move selection · Home/End · g/G jump"),
            Line::from("Tab · f         cycle view (All → Active → Completed → Failed)"),
            Line::from("/               search by filename/URL (Enter apply · Esc clear)"),
            Line::from("s               cycle sort (created/name/size/speed/progress/priority)"),
            Line::from("a               add a download by URL"),
            Line::from("p / r           pause / resume selected task"),
            Line::from("t               retry a failed or cancelled task"),
            Line::from("+ / -           raise / lower selected task priority"),
            Line::from("d               delete selected task (y to confirm)"),
            Line::from("c               remove ALL completed tasks in this view (y to confirm)"),
            Line::from("R               refresh now"),
            Line::from("?               toggle this help"),
            Line::from(""),
            Line::from(Span::styled(
                " Closing the TUI never stops downloads. ",
                Style::default().fg(Color::DarkGray),
            )),
        ];
        frame.render_widget(
            Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" Help ")),
            area,
        );
    }
}

fn centered_rect(
    percent_x: u16,
    percent_y: u16,
    area: ratatui::layout::Rect,
) -> ratatui::layout::Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

fn truncate(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s.to_string()
    } else {
        let keep = max.saturating_sub(1);
        format!("{}…", chars[..keep].iter().collect::<String>())
    }
}

struct TerminalGuard;

impl TerminalGuard {
    fn new() -> std::io::Result<(Self, Terminal<CrosstermBackend<Stdout>>)> {
        enable_raw_mode()?;
        stdout().execute(EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout());
        let terminal = Terminal::new(backend)?;
        Ok((Self, terminal))
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = stdout().execute(LeaveAlternateScreen);
    }
}

/// Launches the interactive terminal UI against `client`.
/// Returns when the user quits (q / Ctrl-C).
pub async fn run(client: AriaFlowClient) -> std::io::Result<()> {
    let mut app = App::new(client);
    app.refresh().await;

    let (_guard, mut terminal) = TerminalGuard::new()?;
    let mut last_refresh = Instant::now();

    loop {
        terminal.draw(|f| ui(f, &mut app))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match &mut app.mode {
                    InputMode::Adding(buffer) => match key.code {
                        KeyCode::Esc => app.mode = InputMode::Normal,
                        KeyCode::Enter => {
                            let url = std::mem::take(buffer);
                            app.mode = InputMode::Normal;
                            app.submit_add(url).await;
                        }
                        KeyCode::Backspace => {
                            buffer.pop();
                        }
                        KeyCode::Char(c) => buffer.push(c),
                        _ => {}
                    },
                    InputMode::ConfirmDelete(_) => match key.code {
                        KeyCode::Char('y') | KeyCode::Char('Y') => app.confirm_delete().await,
                        _ => app.mode = InputMode::Normal,
                    },
                    InputMode::ConfirmClear => match key.code {
                        KeyCode::Char('y') | KeyCode::Char('Y') => {
                            app.mode = InputMode::Normal;
                            app.clear_completed().await;
                        }
                        _ => app.mode = InputMode::Normal,
                    },
                    InputMode::Search(buffer) => match key.code {
                        KeyCode::Esc => {
                            app.mode = InputMode::Normal;
                            app.search = None;
                            app.refresh().await;
                        }
                        KeyCode::Enter => {
                            let query = buffer.trim().to_string();
                            app.mode = InputMode::Normal;
                            app.search = if query.is_empty() { None } else { Some(query) };
                            app.refresh().await;
                        }
                        KeyCode::Backspace => {
                            buffer.pop();
                        }
                        KeyCode::Char(c) => buffer.push(c),
                        _ => {}
                    },
                    InputMode::Normal => {
                        // Help overlay swallows keys; only ?/Esc/q close it.
                        if app.show_help {
                            match key.code {
                                KeyCode::Char('?') | KeyCode::Esc | KeyCode::Char('q') => {
                                    app.show_help = false
                                }
                                _ => {}
                            }
                            continue;
                        }
                        match key.code {
                            KeyCode::Char('q') => break,
                            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                break
                            }
                            KeyCode::Up | KeyCode::Char('k') => app.move_selection(-1),
                            KeyCode::Down | KeyCode::Char('j') => app.move_selection(1),
                            KeyCode::Home | KeyCode::Char('g') => app.jump_selection(false),
                            KeyCode::End | KeyCode::Char('G') => app.jump_selection(true),
                            KeyCode::Tab | KeyCode::Char('f') => {
                                app.filter = app.filter.next();
                                app.refresh().await;
                            }
                            KeyCode::Char('/') => {
                                app.mode = InputMode::Search(app.search.clone().unwrap_or_default())
                            }
                            KeyCode::Char('s') => {
                                app.cycle_sort();
                                app.refresh().await;
                            }
                            KeyCode::Char('a') => app.mode = InputMode::Adding(String::new()),
                            KeyCode::Char('p') | KeyCode::Char('r') => {
                                app.toggle_pause_resume().await
                            }
                            KeyCode::Char('t') => app.retry_selected().await,
                            KeyCode::Char('+') | KeyCode::Char('=') => app.bump_priority(1).await,
                            KeyCode::Char('-') | KeyCode::Char('_') => app.bump_priority(-1).await,
                            KeyCode::Char('d') => app.delete_selected().await,
                            KeyCode::Char('c') => app.mode = InputMode::ConfirmClear,
                            KeyCode::Char('R') => app.refresh().await,
                            KeyCode::Char('?') => app.show_help = true,
                            _ => {}
                        }
                    }
                }
            }
        }

        if last_refresh.elapsed() >= Duration::from_secs(1) {
            app.refresh().await;
            last_refresh = Instant::now();
        }
    }

    Ok(())
}
