//! Rendering for the race-control TUI.

use crate::tui::app::{App, Mode};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};

pub fn render(frame: &mut Frame, app: &App) {
    let [main_area, help_area] =
        Layout::vertical([Constraint::Min(10), Constraint::Length(1)]).areas(frame.area());

    let [left, right] =
        Layout::horizontal([Constraint::Length(38), Constraint::Min(30)]).areas(main_area);

    let [decoder_area, riders_area, clients_area] = Layout::vertical([
        Constraint::Length(9),
        Constraint::Min(6),
        Constraint::Length(7),
    ])
    .areas(left);

    render_decoder(frame, app, decoder_area);
    render_riders(frame, app, riders_area);
    render_clients(frame, app, clients_area);
    render_log(frame, app, right);
    render_help(frame, app, help_area);

    if let Mode::Edit { selected, input } = &app.mode {
        render_edit_popup(frame, app, *selected, input.as_deref());
    }
}

fn decoder_id_string(id: u32) -> String {
    let b = id.to_le_bytes();
    format!("{:02X}{:02X}{:02X}{:02X}", b[0], b[1], b[2], b[3])
}

fn render_decoder(frame: &mut Frame, app: &App, area: Rect) {
    let s = &app.settings;
    let status_line = if app.snapshot.status_paused {
        Span::styled("PAUSED", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
    } else {
        Span::styled(
            format!("every {}s", s.status_interval_s),
            Style::default().fg(Color::Green),
        )
    };

    let lines = vec![
        Line::from(format!("ID        {}", decoder_id_string(s.decoder_id))),
        Line::from(format!("Noise     {}", s.noise)),
        Line::from(format!(
            "Temp      {:.1} °C",
            s.temperature_x10 as f64 / 10.0
        )),
        Line::from(format!(
            "GPS       {}  sats {}",
            if s.gps_fix { "fix" } else { "no fix" },
            s.satellites
        )),
        Line::from(vec![Span::raw("STATUS    "), status_line]),
        Line::from(format!("Passings  {}", app.snapshot.passing_number)),
        Line::from(format!("Gate ID   {}", s.gate_transponder)),
    ];

    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" Decoder ")),
        area,
    );
}

fn render_riders(frame: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .riders
        .iter()
        .map(|r| {
            let style = if r.enabled {
                Style::default()
            } else {
                Style::default().fg(Color::DarkGray)
            };
            ListItem::new(Line::styled(
                format!(
                    "[{}] {} {:>9} {}",
                    r.slot,
                    r.string_id,
                    r.transponder_id,
                    if r.enabled { "" } else { "(off)" }
                ),
                style,
            ))
        })
        .collect();

    frame.render_widget(
        List::new(items).block(Block::default().borders(Borders::ALL).title(" Riders ")),
        area,
    );
}

fn render_clients(frame: &mut Frame, app: &App, area: Rect) {
    let clients = app.registry.snapshot();
    let items: Vec<ListItem> = if clients.is_empty() {
        vec![ListItem::new(Line::styled(
            "none connected",
            Style::default().fg(Color::DarkGray),
        ))]
    } else {
        clients
            .iter()
            .map(|c| {
                ListItem::new(format!(
                    "{}  {} msgs  {} B",
                    c.addr, c.messages_sent, c.bytes_sent
                ))
            })
            .collect()
    };

    let title = match app.chunk_size {
        Some(size) => format!(" Clients (port {}, {}B chunks) ", app.port, size),
        None => format!(" Clients (port {}) ", app.port),
    };
    frame.render_widget(
        List::new(items).block(Block::default().borders(Borders::ALL).title(title)),
        area,
    );
}

fn render_log(frame: &mut Frame, app: &App, area: Rect) {
    let visible_rows = area.height.saturating_sub(2) as usize;
    let lines: Vec<Line> = app
        .log
        .iter()
        .rev()
        .take(visible_rows)
        .rev()
        .map(|entry| {
            let style = if entry.contains("WARN") || entry.contains("ERROR") {
                Style::default().fg(Color::Yellow)
            } else if entry.contains("FAULT") {
                Style::default().fg(Color::Magenta)
            } else {
                Style::default()
            };
            Line::styled(entry.clone(), style)
        })
        .collect();

    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" Event Log ")),
        area,
    );
}

fn render_help(frame: &mut Frame, app: &App, area: Rect) {
    let help = match &app.mode {
        Mode::Normal => {
            " [g]ate  [1-8] rider  [r]ace  [s]tatus  [p]ause-status  faults: [c]rc [x] garbage [t]runcated  [e]dit  [q]uit"
        }
        Mode::Edit { input: None, .. } => {
            " ↑/↓ select  [Enter] edit  [Space] toggle  [Esc] back"
        }
        Mode::Edit { input: Some(_), .. } => " type value  [Enter] apply  [Esc] cancel",
    };
    frame.render_widget(
        Paragraph::new(help).style(Style::default().fg(Color::Cyan)),
        area,
    );
}

fn render_edit_popup(frame: &mut Frame, app: &App, selected: usize, input: Option<&str>) {
    let area = centered_rect(60, 80, frame.area());
    frame.render_widget(Clear, area);

    let entries = app.edit_entries();
    let items: Vec<ListItem> = entries
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let marker = if i == selected { "▶ " } else { "  " };
            let value = if i == selected {
                match input {
                    Some(buffer) => format!("{}▏", buffer),
                    None => entry.value.clone(),
                }
            } else {
                entry.value.clone()
            };
            let style = if i == selected {
                Style::default().add_modifier(Modifier::BOLD).fg(Color::Cyan)
            } else {
                Style::default()
            };
            ListItem::new(Line::styled(
                format!("{}{:<26} {}", marker, entry.label, value),
                style,
            ))
        })
        .collect();

    frame.render_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Edit settings (saved to SQLite on apply) "),
        ),
        area,
    );
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1]);
    horizontal[1]
}
