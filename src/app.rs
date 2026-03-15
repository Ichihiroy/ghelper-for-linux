// ── GHelper Linux — TUI
// Layout inspired by spotify-tui: sidebar + content + status bar.
// Single phosphor-green accent, near-black ground, minimal chrome.

use std::io::stdout;
use std::time::{Duration, Instant};

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Gauge, List, ListItem, ListState, Paragraph},
    Frame, Terminal,
};

use crate::{
    battery::{BatteryManager, ChargeStatus},
    config::Config,
    system::SystemInfo,
};

// ── Palette ───────────────────────────────────────────────────────────────────

const GREEN:  Color = Color::Rgb(61, 220, 132);
const TEXT:   Color = Color::Rgb(255, 255, 255);   // pure white for primary text
const DIM:    Color = Color::Rgb(170, 170, 170);   // light gray — clearly readable
const FAINT:  Color = Color::Rgb(110, 110, 110);   // medium gray — visible but secondary
const DANGER: Color = Color::Rgb(220, 70, 70);
const LINE:   Color = Color::Rgb(50, 50, 50);      // slightly brighter borders

fn g()   -> Style { Style::default().fg(GREEN) }
fn txt() -> Style { Style::default().fg(TEXT) }
fn dim() -> Style { Style::default().fg(DIM) }
fn fnt() -> Style { Style::default().fg(FAINT) }
fn dng() -> Style { Style::default().fg(DANGER) }
fn bg()  -> Style { Style::default().fg(GREEN).add_modifier(Modifier::BOLD) }
fn line_border() -> Style { Style::default().fg(LINE) }

// ── State ─────────────────────────────────────────────────────────────────────

#[derive(PartialEq, Clone, Copy)]
pub enum Focus { Sidebar, Content }

#[derive(Clone)]
pub enum Status { None, Ok(String), Err(String) }

pub struct TuiApp {
    pub battery:         BatteryManager,
    pub system:          SystemInfo,
    pub config:          Config,
    pub active_tab:      usize,   // 0 battery  1 system  2 settings
    pub focus:           Focus,
    pub desired_limit:   u8,
    pub status:          Status,
    pub status_until:    Option<Instant>,
    pub last_refresh:    Instant,
    pub should_quit:     bool,
    pub settings_cursor: usize,
}

impl TuiApp {
    fn new() -> Self {
        let config  = Config::load();
        let battery = BatteryManager::new();
        let desired = config.charge_limit;
        let app = Self {
            desired_limit: desired,
            battery,
            system: SystemInfo::new(),
            config,
            active_tab: 0,
            focus: Focus::Sidebar,
            status: Status::None,
            status_until: Option::None,
            last_refresh: Instant::now(),
            should_quit: false,
            settings_cursor: 0,
        };
        if app.config.auto_apply_on_start {
            let _ = app.battery.set_charge_limit(app.desired_limit);
        }
        app
    }

    fn refresh(&mut self) {
        self.battery.refresh();
        self.system.refresh();
        self.last_refresh = Instant::now();
    }

    fn expire_status(&mut self) {
        if let Some(until) = self.status_until {
            if Instant::now() > until {
                self.status = Status::None;
                self.status_until = Option::None;
            }
        }
    }

    fn ok(&mut self, msg: impl Into<String>, secs: u64) {
        self.status = Status::Ok(msg.into());
        self.status_until = Some(Instant::now() + Duration::from_secs(secs));
    }

    fn err(&mut self, msg: impl Into<String>, secs: u64) {
        self.status = Status::Err(msg.into());
        self.status_until = Some(Instant::now() + Duration::from_secs(secs));
    }

    fn do_apply(&mut self) {
        match self.battery.set_charge_limit(self.desired_limit) {
            Ok(()) => {
                self.battery.refresh();
                self.config.charge_limit = self.desired_limit;
                self.config.save();
                self.ok(format!("limit set to {}%", self.desired_limit), 4);
            }
            Err(e) => self.err(e, 8),
        }
    }

    fn do_setup(&mut self) {
        match self.battery.run_setup(self.desired_limit) {
            Ok(()) => {
                self.battery.refresh();
                self.config.persistent_limit = true;
                self.config.save();
                self.ok("setup complete — persistent limit active", 6);
            }
            Err(e) => self.err(e, 10),
        }
    }

    fn do_persist(&mut self) {
        match self.battery.update_persistent_limit(self.desired_limit) {
            Ok(()) => self.ok("persistent limit updated", 4),
            Err(e) => self.err(e, 8),
        }
    }

    // ── Input ─────────────────────────────────────────────────────────────────

    fn handle_key(&mut self, code: KeyCode, mods: KeyModifiers) {
        // Global
        match code {
            KeyCode::Char('q') | KeyCode::Char('Q') => { self.should_quit = true; return; }
            KeyCode::Char('r') | KeyCode::Char('R') => { self.refresh(); return; }
            KeyCode::Tab => {
                self.focus = if self.focus == Focus::Sidebar { Focus::Content } else { Focus::Sidebar };
                return;
            }
            _ => {}
        }
        match self.focus {
            Focus::Sidebar  => self.sidebar_key(code),
            Focus::Content  => self.content_key(code, mods),
        }
    }

    fn sidebar_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Down  | KeyCode::Char('j') => self.active_tab = (self.active_tab + 1).min(2),
            KeyCode::Up    | KeyCode::Char('k') => { if self.active_tab > 0 { self.active_tab -= 1; } }
            KeyCode::Right | KeyCode::Enter | KeyCode::Char('l') => self.focus = Focus::Content,
            _ => {}
        }
    }

    fn content_key(&mut self, code: KeyCode, mods: KeyModifiers) {
        match code {
            KeyCode::Esc | KeyCode::BackTab => { self.focus = Focus::Sidebar; return; }
            _ => {}
        }
        match self.active_tab {
            0 => self.battery_key(code, mods),
            1 => {} // system tab — read-only
            2 => self.settings_key(code),
            _ => {}
        }
    }

    fn battery_key(&mut self, code: KeyCode, mods: KeyModifiers) {
        let step: u8 = if mods.contains(KeyModifiers::SHIFT) { 5 } else { 1 };
        match code {
            KeyCode::Left  => self.desired_limit = self.desired_limit.saturating_sub(step).max(20),
            KeyCode::Right => self.desired_limit = (self.desired_limit as u16 + step as u16).min(100) as u8,
            KeyCode::Char('1') => self.desired_limit = 60,
            KeyCode::Char('2') => self.desired_limit = 80,
            KeyCode::Char('3') => self.desired_limit = 100,
            KeyCode::Enter | KeyCode::Char('a') => self.do_apply(),
            KeyCode::Char('s') => {
                if self.battery.info.persistent_enabled { self.do_persist(); } else { self.do_setup(); }
            }
            _ => {}
        }
    }

    fn settings_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Down  | KeyCode::Char('j') => self.settings_cursor = (self.settings_cursor + 1).min(3),
            KeyCode::Up    | KeyCode::Char('k') => { if self.settings_cursor > 0 { self.settings_cursor -= 1; } }
            KeyCode::Left  => match self.settings_cursor {
                0 => { self.config.charge_limit = self.config.charge_limit.saturating_sub(1).max(20); self.desired_limit = self.config.charge_limit; self.config.save(); }
                2 => { if self.config.refresh_secs > 1 { self.config.refresh_secs -= 1; self.config.save(); } }
                _ => {}
            },
            KeyCode::Right => match self.settings_cursor {
                0 => { self.config.charge_limit = self.config.charge_limit.saturating_add(1).min(100); self.desired_limit = self.config.charge_limit; self.config.save(); }
                2 => { if self.config.refresh_secs < 60 { self.config.refresh_secs += 1; self.config.save(); } }
                _ => {}
            },
            KeyCode::Enter | KeyCode::Char(' ') => match self.settings_cursor {
                1 => { self.config.auto_apply_on_start = !self.config.auto_apply_on_start; self.config.save(); }
                3 => self.do_setup(),
                _ => {}
            },
            _ => {}
        }
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    // Ensure terminal is restored even on panic
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(stdout(), LeaveAlternateScreen, cursor::Show);
        original_hook(info);
    }));

    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen, cursor::Hide)?;

    let backend  = CrosstermBackend::new(stdout());
    let mut term = Terminal::new(backend)?;
    let mut app  = TuiApp::new();

    loop {
        term.draw(|f| render(f, &app))?;

        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(KeyEvent { code, kind: KeyEventKind::Press, modifiers, .. }) = event::read()? {
                app.handle_key(code, modifiers);
                if app.should_quit { break; }
            }
        }

        if app.last_refresh.elapsed() >= Duration::from_secs(app.config.refresh_secs) {
            app.refresh();
        }
        app.expire_status();
    }

    disable_raw_mode()?;
    execute!(term.backend_mut(), LeaveAlternateScreen, cursor::Show)?;
    term.show_cursor()?;
    Ok(())
}

// ── Top-level render ──────────────────────────────────────────────────────────

fn render(f: &mut Frame, app: &TuiApp) {
    let area = f.area();

    // Outer vertical: title | main | statusbar
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
    ]).split(area);

    render_titlebar(f, app, rows[0]);
    render_statusbar(f, app, rows[2]);

    // Main horizontal: sidebar | content
    let cols = Layout::horizontal([
        Constraint::Length(18),
        Constraint::Fill(1),
    ]).split(rows[1]);

    render_sidebar(f, app, cols[0]);

    match app.active_tab {
        0 => render_battery(f, app, cols[1]),
        1 => render_system(f, app, cols[1]),
        2 => render_settings(f, app, cols[1]),
        _ => {}
    }
}

// ── Title bar ─────────────────────────────────────────────────────────────────

fn render_titlebar(f: &mut Frame, app: &TuiApp, area: Rect) {
    let bat = &app.battery.info;
    let sys = &app.system;
    let bg  = Style::default().bg(Color::Rgb(11, 11, 11));

    // Left side
    let mut left: Vec<Span> = vec![
        Span::styled("  ghelper linux", Style::default().fg(GREEN).add_modifier(Modifier::BOLD).bg(Color::Rgb(11, 11, 11))),
        Span::styled("  ·  ", fnt().bg(Color::Rgb(11, 11, 11))),
        Span::styled(
            format!("bat {}%", bat.capacity),
            if bat.status.is_plugged() { g() } else if bat.capacity < 20 { dng() } else { dim() }
                .bg(Color::Rgb(11, 11, 11)),
        ),
    ];
    if let Some(t) = sys.cpu_temp_c {
        let col = if t >= 80.0 { DANGER } else { DIM };
        left.push(Span::styled("  ·  ", fnt().bg(Color::Rgb(11, 11, 11))));
        left.push(Span::styled(format!("cpu {t:.0}°c"), Style::default().fg(col).bg(Color::Rgb(11, 11, 11))));
    }
    if let Some(t) = sys.gpu_temp_c {
        let col = if t >= 85.0 { DANGER } else { DIM };
        left.push(Span::styled("  ·  ", fnt().bg(Color::Rgb(11, 11, 11))));
        left.push(Span::styled(format!("gpu {t:.0}°c"), Style::default().fg(col).bg(Color::Rgb(11, 11, 11))));
    }

    f.render_widget(Paragraph::new(Line::from(left)).style(bg), area);

    // Right side — AC status
    let ac = if sys.ac_connected { "ac  " } else { "bat  " };
    f.render_widget(
        Paragraph::new(Span::styled(ac, dim().bg(Color::Rgb(11, 11, 11))))
            .alignment(Alignment::Right)
            .style(bg),
        area,
    );
}

// ── Sidebar ───────────────────────────────────────────────────────────────────

fn render_sidebar(f: &mut Frame, app: &TuiApp, area: Rect) {
    let focused  = app.focus == Focus::Sidebar;
    let bdr      = if focused { Color::Rgb(45, 45, 45) } else { LINE };
    let tabs     = ["battery", "system", "settings"];

    let items: Vec<ListItem> = tabs.iter()
        .map(|&t| ListItem::new(format!("  {t}")).style(dim()))
        .collect();

    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(bdr));

    let list = List::new(items)
        .block(block)
        .highlight_style(bg())
        .highlight_symbol("▶ ");

    let mut state = ListState::default();
    state.select(Some(app.active_tab));
    f.render_stateful_widget(list, area, &mut state);
}

// ── Battery tab ───────────────────────────────────────────────────────────────

fn render_battery(f: &mut Frame, app: &TuiApp, area: Rect) {
    let bat = &app.battery.info;
    let focused = app.focus == Focus::Content;

    let rows = Layout::vertical([
        Constraint::Length(5),  // status
        Constraint::Length(8),  // charge limit
        Constraint::Length(5),  // details
        Constraint::Fill(1),
    ]).split(area);

    // ── Status block ─────────────────────────────────────────────────────────
    {
        let bdr = if focused { Color::Rgb(40, 40, 40) } else { LINE };
        let block = Block::default()
            .title(Span::styled(" battery status ", dim()))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(bdr));
        let inner = block.inner(rows[0]);
        f.render_widget(block, rows[0]);

        let cells = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ]).split(inner);

        let arc = if bat.status.is_plugged() { GREEN }
                  else if bat.capacity < 20   { DANGER }
                  else                         { GREEN };

        let gauge = Gauge::default()
            .gauge_style(Style::default().fg(arc).bg(Color::Rgb(22, 22, 22)))
            .percent(bat.capacity as u16)
            .label(Span::styled(
                format!("{:3}%  {}", bat.capacity, bat.status.label()),
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
            ));
        f.render_widget(gauge, cells[0]);

        // Stats row
        let mut spans: Vec<Span> = vec![];
        macro_rules! stat {
            ($label:expr, $val:expr) => {
                spans.push(Span::styled($label, dim()));
                spans.push(Span::styled(format!("{}  ", $val), txt()));
            };
        }
        if let Some(v) = bat.voltage_v { stat!("voltage ", format!("{v:.1}V")); }
        if let Some(c) = bat.current_a { stat!("current ", format!("{:.1}A", c.abs())); }
        if let Some(p) = bat.power_w   { stat!("power ",   format!("{p:.1}W")); }
        if let Some(h) = bat.time_remaining_h() {
            let lbl = if matches!(bat.status, ChargeStatus::Charging) { "to full " } else { "remaining " };
            let hh = h as u32;
            let mm = ((h - hh as f32) * 60.0) as u32;
            stat!(lbl, format!("{hh}h {mm:02}m"));
        }
        f.render_widget(Paragraph::new(Line::from(spans)), cells[2]);
    }

    // ── Charge limit block ────────────────────────────────────────────────────
    {
        let changed  = app.desired_limit != bat.charge_limit;
        let bdr      = if focused { Color::Rgb(35, 60, 45) } else { LINE };
        let title_col = if focused { GREEN } else { DIM };

        let block = Block::default()
            .title(Span::styled(
                " charge limit ",
                Style::default().fg(title_col),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(bdr));
        let inner = block.inner(rows[1]);
        f.render_widget(block, rows[1]);

        let cells = Layout::vertical([
            Constraint::Length(1), // gauge
            Constraint::Length(1), // spacer
            Constraint::Length(1), // presets
            Constraint::Length(1), // spacer
            Constraint::Length(1), // action / status
            Constraint::Fill(1),
        ]).split(inner);

        // Gauge
        let g_col   = if changed { GREEN } else { Color::Rgb(48, 48, 48) };
        let hint    = if focused { "  ←→ adjust  shift+←→ ±5" } else { "  (focus with tab)" };
        let gauge = Gauge::default()
            .gauge_style(Style::default().fg(g_col).bg(Color::Rgb(22, 22, 22)))
            .percent(app.desired_limit as u16)
            .label(Span::styled(
                format!("{:3}%{hint}", app.desired_limit),
                if changed { txt() } else { dim() },
            ));
        f.render_widget(gauge, cells[0]);

        // Preset buttons
        let mut preset_spans: Vec<Span> = vec![Span::styled("  presets  ", fnt())];
        for (i, &pct) in [60u8, 80, 100].iter().enumerate() {
            let active = app.desired_limit == pct;
            let s = if active { format!("[{pct}%]") } else { format!(" {pct}% ") };
            preset_spans.push(Span::styled(s, if active { bg() } else { dim() }));
            if i < 2 { preset_spans.push(Span::styled("  ", fnt())); }
        }
        preset_spans.push(Span::styled("   1 / 2 / 3", fnt()));
        f.render_widget(Paragraph::new(Line::from(preset_spans)), cells[2]);

        // Action line or status
        let action_widget = match &app.status {
            Status::Ok(msg)  => Paragraph::new(Line::from(vec![
                Span::styled("  ✓  ", g()),
                Span::styled(msg.as_str(), g()),
            ])),
            Status::Err(msg) => Paragraph::new(Line::from(vec![
                Span::styled("  ✗  ", dng()),
                Span::styled(msg.as_str(), dng()),
            ])),
            Status::None => {
                let apply_style = if changed { g() } else { dim() };
                let persist_label = if app.battery.info.persistent_enabled { "[s] update persist" } else { "[s] setup persist" };
                Paragraph::new(Line::from(vec![
                    Span::styled("  [a/↵] apply  ", apply_style),
                    Span::styled(persist_label, fnt()),
                ]))
            }
        };
        f.render_widget(action_widget, cells[4]);
    }

    // ── Details block ─────────────────────────────────────────────────────────
    {
        let block = Block::default()
            .title(Span::styled(" details ", fnt()))
            .borders(Borders::ALL)
            .border_style(line_border());
        let inner = block.inner(rows[2]);
        f.render_widget(block, rows[2]);

        let cells = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
        ]).split(inner);

        // Row 1: energy + health
        let mut r1: Vec<Span> = vec![];
        if let Some(en) = bat.energy_now_wh  { r1.push(Span::styled("energy now ",  dim())); r1.push(Span::styled(format!("{en:.1}Wh  "), txt())); }
        if let Some(ef) = bat.energy_full_wh { r1.push(Span::styled("full ",        dim())); r1.push(Span::styled(format!("{ef:.1}Wh  "), txt())); }
        if let Some(h)  = bat.health_percent() {
            let hc = if h > 80.0 { GREEN } else if h > 60.0 { DIM } else { DANGER };
            r1.push(Span::styled("health ", dim()));
            r1.push(Span::styled(format!("{h:.0}%"), Style::default().fg(hc)));
        }
        f.render_widget(Paragraph::new(Line::from(r1)), cells[0]);

        // Row 2: cycles + tech + device + persistent
        let mut r2: Vec<Span> = vec![];
        if let Some(c) = bat.cycle_count { r2.push(Span::styled("cycles ", dim())); r2.push(Span::styled(format!("{c}  "), txt())); }
        if let Some(t) = &bat.technology { r2.push(Span::styled("tech ", dim())); r2.push(Span::styled(format!("{t}  "), txt())); }
        r2.push(Span::styled("device ", dim()));
        r2.push(Span::styled(format!("{}  ", app.battery.bat_name()), txt()));
        r2.push(Span::styled("persistent ", dim()));
        r2.push(Span::styled(
            if bat.persistent_enabled { "active" } else { "not set" },
            if bat.persistent_enabled { g() } else { dim() },
        ));
        f.render_widget(Paragraph::new(Line::from(r2)), cells[1]);
    }
}

// ── System tab ────────────────────────────────────────────────────────────────

fn render_system(f: &mut Frame, app: &TuiApp, area: Rect) {
    let sys = &app.system;

    let rows = Layout::vertical([
        Constraint::Length(5),
        Constraint::Length(5),
        Constraint::Fill(1),
    ]).split(area);

    // CPU / GPU temps
    {
        let block = Block::default()
            .title(Span::styled(" processor ", fnt()))
            .borders(Borders::ALL)
            .border_style(line_border());
        let inner = block.inner(rows[0]);
        f.render_widget(block, rows[0]);

        let cells = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ]).split(inner);

        if let Some(ref m) = sys.cpu_model {
            let s: String = m.chars().take((inner.width as usize).saturating_sub(2)).collect();
            f.render_widget(Paragraph::new(Span::styled(s, dim())), cells[0]);
        }

        let mut spans: Vec<Span> = vec![];
        if let Some(t) = sys.cpu_temp_c {
            let col = if t >= 80.0 { DANGER } else { TEXT };
            spans.push(Span::styled("cpu ", dim()));
            spans.push(Span::styled(format!("{t:.0}°C"), Style::default().fg(col)));
        }
        if let Some(t) = sys.gpu_temp_c {
            let col = if t >= 85.0 { DANGER } else { TEXT };
            spans.push(Span::styled("   gpu ", dim()));
            spans.push(Span::styled(format!("{t:.0}°C"), Style::default().fg(col)));
        }
        f.render_widget(Paragraph::new(Line::from(spans)), cells[2]);
    }

    // Memory
    {
        let block = Block::default()
            .title(Span::styled(" memory ", fnt()))
            .borders(Borders::ALL)
            .border_style(line_border());
        let inner = block.inner(rows[1]);
        f.render_widget(block, rows[1]);

        if let (Some(total), Some(used), Some(avail)) =
            (sys.mem_total_mb, sys.mem_used_mb, sys.mem_available_mb)
        {
            let pct = sys.mem_used_percent().unwrap_or(0.0);
            let mc  = if pct > 85.0 { DANGER } else { GREEN };

            let cells = Layout::vertical([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ]).split(inner);

            let gauge = Gauge::default()
                .gauge_style(Style::default().fg(mc).bg(Color::Rgb(22, 22, 22)))
                .percent(pct as u16)
                .label(Span::styled(format!("{pct:.0}%  ({used} / {total} MB)"), txt()));
            f.render_widget(gauge, cells[0]);

            f.render_widget(Paragraph::new(Line::from(vec![
                Span::styled("used ",      dim()), Span::styled(format!("{used} MB  "),  txt()),
                Span::styled("available ", dim()), Span::styled(format!("{avail} MB  "), txt()),
                Span::styled("total ",     dim()), Span::styled(format!("{total} MB"),   txt()),
            ])), cells[2]);
        } else {
            f.render_widget(Paragraph::new(Span::styled("unavailable", dim())), inner);
        }
    }

    // Thermal zones
    if !sys.thermal_zones.is_empty() {
        let block = Block::default()
            .title(Span::styled(" thermal zones ", fnt()))
            .borders(Borders::ALL)
            .border_style(line_border());
        let inner = block.inner(rows[2]);
        f.render_widget(block, rows[2]);

        let cols = Layout::horizontal([
            Constraint::Ratio(1, 2),
            Constraint::Ratio(1, 2),
        ]).split(inner);

        let half = (sys.thermal_zones.len() + 1) / 2;
        for (ci, chunk) in sys.thermal_zones.chunks(half).enumerate() {
            if ci >= 2 { break; }
            let lines: Vec<Line> = chunk.iter().map(|z| {
                let tc = if z.temp_c >= 80.0 { DANGER } else if z.temp_c >= 65.0 { DIM } else { TEXT };
                Line::from(vec![
                    Span::styled(format!("{:<22}", z.name), dim()),
                    Span::styled(format!("{:.1}°C", z.temp_c), Style::default().fg(tc)),
                ])
            }).collect();
            f.render_widget(Paragraph::new(Text::from(lines)), cols[ci]);
        }
    }
}

// ── Settings tab ──────────────────────────────────────────────────────────────

fn render_settings(f: &mut Frame, app: &TuiApp, area: Rect) {
    let focused = app.focus == Focus::Content;

    let block = Block::default()
        .title(Span::styled(" settings ", fnt()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if focused { Color::Rgb(35, 35, 35) } else { LINE }));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::vertical([
        Constraint::Length(1), // header
        Constraint::Length(1), // spacer
        Constraint::Fill(1),   // settings list
    ]).split(inner);

    // Column header
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!("  {:<32}", "setting"), fnt()),
            Span::styled(format!("{:<16}", "value"), fnt()),
            Span::styled("key", fnt()),
        ])),
        rows[0],
    );

    let settings: &[(&str, String, &str)] = &[
        ("default charge limit",  format!("{}%",  app.config.charge_limit),           "← →"),
        ("apply on startup",      (if app.config.auto_apply_on_start {"on"} else {"off"}).to_string(), "space/↵"),
        ("refresh interval",      format!("{}s",  app.config.refresh_secs),            "← →"),
        ("persistence setup",     (if app.battery.info.persistent_enabled {"configured"} else {"not configured"}).to_string(), "↵"),
    ];

    let mut lines: Vec<Line> = vec![];
    for (i, (label, value, key)) in settings.iter().enumerate() {
        let sel = focused && i == app.settings_cursor;
        let cursor = if sel { "▶ " } else { "  " };
        lines.push(Line::from(vec![
            Span::styled(cursor,                     if sel { g() } else { fnt() }),
            Span::styled(format!("{:<30}", label),   if sel { txt() } else { dim() }),
            Span::styled(format!("{:<16}", value),   if sel { bg() } else { txt() }),
            Span::styled(format!("[{key}]"),          fnt()),
        ]));
        lines.push(Line::from("")); // breathing room between rows
    }

    // Status at bottom
    match &app.status {
        Status::Ok(msg)  => lines.push(Line::from(vec![Span::styled(format!("  ✓  {msg}"), g())])),
        Status::Err(msg) => lines.push(Line::from(vec![Span::styled(format!("  ✗  {msg}"), dng())])),
        Status::None     => {}
    }

    f.render_widget(Paragraph::new(Text::from(lines)), rows[2]);
}

// ── Status bar (keybindings) ──────────────────────────────────────────────────

fn render_statusbar(f: &mut Frame, app: &TuiApp, area: Rect) {
    let keys = match (app.active_tab, app.focus) {
        (0, Focus::Content) => "  q quit   tab→sidebar   ←→ limit   shift+←→ ±5   1/2/3 presets   a/↵ apply   s setup/persist   r refresh",
        (1, Focus::Content) => "  q quit   tab→sidebar   r refresh",
        (2, Focus::Content) => "  q quit   tab→sidebar   j/k navigate   ←→ adjust   space/↵ toggle   r refresh",
        (_, Focus::Sidebar)  => "  q quit   j/k navigate   ↵/→ select tab   tab→content   r refresh",
        _                    => "  q quit",
    };
    f.render_widget(
        Paragraph::new(Span::styled(keys, fnt()))
            .style(Style::default().bg(Color::Rgb(10, 10, 10))),
        area,
    );
}
