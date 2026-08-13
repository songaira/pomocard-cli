//! Frame composition: the agent-harness chrome from the landing page mock,
//! plus view routing and overlays (palette, help, toast).

pub mod views;

use ratatui::prelude::*;
use ratatui::widgets::{Block, BorderType, Clear, Paragraph, Wrap};

use crate::agent::Entry;
use crate::app::{App, InputMode, View};
use crate::theme::{self, Theme};

pub fn draw(f: &mut Frame, app: &App) {
    let th = Theme::from_name(&app.state.theme);
    let area = f.area();
    f.render_widget(Block::default().style(th.base()), area);

    let outer = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(th.border())
        .style(th.base())
        .title(title_line(&th))
        .title_bottom(hint_line(app, &th).right_aligned());
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .horizontal_margin(1)
    .split(inner);

    tabs(f, app, rows[0], &th);
    status(f, app, rows[1], &th);
    f.render_widget(
        Paragraph::new(Line::styled(
            "─".repeat(rows[2].width as usize),
            th.faint(),
        )),
        rows[2],
    );

    let body = rows[3];
    match app.view {
        View::Agent => views::agent(f, app, body, &th),
        View::Board => views::board(f, app, body, &th),
        View::Analytics => views::analytics(f, app, body, &th),
        View::Coach => views::coach(f, app, body, &th),
        View::Templates => views::templates(f, app, body, &th),
        View::Team => views::team(f, app, body, &th),
        View::Billing => views::billing(f, app, body, &th),
        View::Settings => views::settings(f, app, body, &th),
    }

    prompt(f, app, rows[4], &th);

    if app.palette.is_some() {
        palette(f, app, area, &th);
    }
    if app.help {
        help(f, area, &th);
    }
    if let Some(t) = &app.toast {
        toast(f, &t.title, &t.sub, area, &th);
    }
}

fn title_line<'a>(th: &'a Theme) -> Line<'a> {
    Line::from(vec![
        Span::styled(" ● ● ●  ", th.dim()),
        Span::styled("pomocard agent", th.bold()),
    ])
}

fn hint_line<'a>(app: &App, th: &Theme) -> Line<'a> {
    let keys = match app.mode {
        InputMode::Prompt => " Esc normal · ^K palette · Tab views · ^C quit ",
        InputMode::Normal => " i prompt · ? keys · Tab views · q quit ",
    };
    Line::styled(keys.to_string(), th.dim())
}

fn tabs(f: &mut Frame, app: &App, area: Rect, th: &Theme) {
    let mut spans: Vec<Span> = Vec::new();
    for (i, v) in View::ALL.iter().enumerate() {
        let active = *v == app.view;
        let label = format!(" {} {} ", i + 1, v.title());
        let style = if active { th.inverse() } else { th.dim() };
        spans.push(Span::styled(label, style));
        spans.push(Span::styled(" ", th.base()));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn status(f: &mut Frame, app: &App, area: Rect, th: &Theme) {
    let cols = Layout::horizontal([Constraint::Min(10), Constraint::Length(9)]).split(area);
    let clock = format!(
        "{} {}",
        app.timer.mode.short_label().to_lowercase(),
        app.timer.clock()
    );
    let task = app
        .state
        .active_card()
        .map(|c| c.title.clone())
        .unwrap_or_else(|| "no task pinned".into());
    let left = Line::from(vec![
        Span::styled("session ", th.dim()),
        Span::styled(app.session_id.clone(), th.bold()),
        Span::styled("   model ", th.dim()),
        Span::styled("focus-1", th.bold()),
        Span::styled("   cwd ", th.dim()),
        Span::styled(app.cwd.clone(), th.bold()),
        Span::styled("   ", th.dim()),
        Span::styled(clock, th.bold()),
        Span::styled(if app.timer.running { " ▶" } else { " ‖" }, th.dim()),
        Span::styled("   task ", th.dim()),
        Span::styled(truncate(&task, 28), th.bold()),
    ]);
    f.render_widget(Paragraph::new(left), cols[0]);

    let live = if app.timer.running { "● live" } else { "○ idle" };
    f.render_widget(
        Paragraph::new(Line::styled(live.to_string(), th.dim())).alignment(Alignment::Right),
        cols[1],
    );
}

fn prompt(f: &mut Frame, app: &App, area: Rect, th: &Theme) {
    let line = if app.thinking {
        let spinner = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧'][(app.tick_count as usize) % 8];
        Line::from(vec![
            Span::styled(format!("{} ", theme::PROMPT), th.bold()),
            Span::styled(format!("{spinner} thinking"), th.bold()),
        ])
    } else {
        match app.mode {
            InputMode::Prompt => {
                let (before, after) = split_at_cursor(&app.input, app.cursor);
                let (cursor_char, rest) = take_first(&after);
                Line::from(vec![
                    Span::styled(format!("{} ", theme::PROMPT), th.bold()),
                    Span::styled(before, th.base()),
                    Span::styled(cursor_char, th.inverse()),
                    Span::styled(rest, th.base()),
                ])
            }
            InputMode::Normal => Line::from(vec![
                Span::styled(format!("{} ", theme::PROMPT), th.dim()),
                Span::styled(
                    "normal mode — i to talk to the agent, ? for keys".to_string(),
                    th.faint(),
                ),
            ]),
        }
    };
    f.render_widget(Paragraph::new(line), area);
}

/* ---------------- overlays ---------------- */

fn palette(f: &mut Frame, app: &App, area: Rect, th: &Theme) {
    let Some(p) = &app.palette else { return };
    let items = app.filtered_palette();
    let h = (items.len().min(10) as u16) + 4;
    let rect = centered(area, 72, h.max(6));
    f.render_widget(Clear, rect);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(th.border())
        .style(th.base())
        .title(Span::styled(" command palette ", th.bold()));
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let rows = Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).split(inner);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("❯ ", th.bold()),
            Span::styled(p.query.clone(), th.base()),
            Span::styled("█", th.dim()),
        ])),
        rows[0],
    );

    let view_h = rows[1].height as usize;
    let start = p.sel.saturating_sub(view_h.saturating_sub(1));
    let lines: Vec<Line> = items
        .iter()
        .enumerate()
        .skip(start)
        .take(view_h)
        .map(|(i, a)| {
            let style = if i == p.sel { th.inverse() } else { th.base() };
            let hint = if a.hint.is_empty() {
                String::new()
            } else {
                format!("  {}", a.hint)
            };
            Line::from(vec![
                Span::styled(format!(" {} ", a.label), style),
                Span::styled(hint, th.dim()),
            ])
        })
        .collect();
    let body = if lines.is_empty() {
        vec![Line::styled(
            " No matches. Try “timer”, “board”, or a card name.".to_string(),
            th.dim(),
        )]
    } else {
        lines
    };
    f.render_widget(Paragraph::new(body), rows[1]);
}

fn help(f: &mut Frame, area: Rect, th: &Theme) {
    let rect = centered(area, 76, 24);
    f.render_widget(Clear, rect);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(th.border())
        .style(th.base())
        .title(Span::styled(" keys & commands ", th.bold()))
        .title_bottom(Line::styled(" any key closes ", th.dim()).right_aligned());
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let k = |key: &str, desc: &str| -> Line<'static> {
        Line::from(vec![
            Span::styled(format!("  {key:<14}"), th.bold()),
            Span::styled(desc.to_string(), th.dim()),
        ])
    };
    let mut lines = vec![
        Line::styled("  TIMER", th.bold()),
        k("space", "start / pause"),
        k("r · n", "reset · skip to next session"),
        k("f · s · l", "focus · short break · long break"),
        Line::raw(""),
        Line::styled("  BOARD", th.bold()),
        k("← ↑ ↓ → / hjkl", "move the selection"),
        k("H · L", "move card to previous / next column"),
        k("J · K", "reorder card inside the column"),
        k("a · x · c", "add · delete · cycle estimate"),
        k("p · e", "pin as current task · rename"),
        Line::raw(""),
        Line::styled("  APP", th.bold()),
        k("i / Enter", "focus the agent prompt (Esc leaves)"),
        k("Tab / 1-8", "switch views"),
        k("^K", "command palette"),
        k("T", "toggle theme"),
        k("PgUp/PgDn", "scroll the transcript"),
        k("q / ^C", "quit (state is saved)"),
        Line::raw(""),
        Line::styled("  TRY TYPING", th.bold()),
        Line::styled(
            "  start a 25m focus block and drop \"Ship the landing page\" into Today".to_string(),
            th.dim(),
        ),
        Line::styled(
            "  move \"Refactor timer module\" to in progress · finish \"Final QA pass\"".to_string(),
            th.dim(),
        ),
        Line::styled(
            "  load the thesis template · stats · set focus 50".to_string(),
            th.dim(),
        ),
    ];
    lines.truncate(inner.height as usize);
    f.render_widget(Paragraph::new(lines), inner);
}

fn toast(f: &mut Frame, title: &str, sub: &str, area: Rect, th: &Theme) {
    let w = (title.len().max(sub.len()) + 6).clamp(20, 46) as u16;
    let h = 4u16;
    if area.width < w + 4 || area.height < h + 4 {
        return;
    }
    let rect = Rect {
        x: area.x + area.width - w - 2,
        y: area.y + area.height - h - 2,
        width: w,
        height: h,
    };
    f.render_widget(Clear, rect);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(th.border())
        .style(th.base());
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    f.render_widget(
        Paragraph::new(vec![
            Line::styled(format!(" {title}"), th.bold()),
            Line::styled(format!(" {sub}"), th.dim()),
        ])
        .wrap(Wrap { trim: true }),
        inner,
    );
}

/* ---------------- helpers ---------------- */

pub fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    }
}

pub fn truncate(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn split_at_cursor(s: &str, cursor: usize) -> (String, String) {
    let idx = cursor.min(s.len());
    (s[..idx].to_string(), s[idx..].to_string())
}

fn take_first(s: &str) -> (String, String) {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => (c.to_string(), chars.collect()),
        None => (" ".to_string(), String::new()),
    }
}

/// Turns the transcript into styled lines that mirror the landing page mock.
pub fn transcript_lines(app: &App, th: &Theme) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    for entry in &app.transcript {
        match entry {
            Entry::You(text) => {
                lines.push(Line::from(vec![
                    Span::styled(format!("{} You    ", theme::PROMPT), th.bold()),
                    Span::styled(text.clone(), th.base()),
                ]));
                lines.push(Line::raw(""));
            }
            Entry::Agent(text) => {
                lines.push(Line::from(vec![
                    Span::styled(format!("{} Agent  ", theme::AGENT), th.dim()),
                    Span::styled(text.clone(), th.base()),
                ]));
                lines.push(Line::raw(""));
            }
            Entry::Tool(tool) => {
                lines.push(Line::from(vec![
                    Span::styled(format!("{} ", theme::BAR), th.faint()),
                    Span::styled(tool.name.clone(), th.bold()),
                    Span::styled(format!("  {}", tool.status()), th.dim()),
                ]));
                for (k, v) in &tool.rows {
                    lines.push(Line::from(vec![
                        Span::styled(format!("{}   ", theme::BAR), th.faint()),
                        Span::styled(format!("{:<9}", k), th.dim()),
                        Span::styled(v.clone(), th.base()),
                    ]));
                }
                lines.push(Line::raw(""));
            }
            Entry::Note(text) => {
                lines.push(Line::styled(format!("· {text}"), th.dim()));
                lines.push(Line::raw(""));
            }
        }
    }
    lines
}

/// Rough wrapped-height estimate so auto-scroll lands on the newest turn.
pub fn wrapped_len(lines: &[Line<'_>], width: u16) -> usize {
    let w = width.max(1) as usize;
    lines
        .iter()
        .map(|l| {
            let len: usize = l.spans.iter().map(|s| s.content.chars().count()).sum();
            (len.max(1) + w - 1) / w
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{App, Palette};
    use crate::data::Data;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn test_app() -> App {
        let path = std::env::temp_dir().join("pomocard-render-test.json");
        let _ = std::fs::remove_file(&path);
        let mut app = App::new(path, Data::embedded()).expect("app boots");
        app.state.stats.minutes = 125;
        app.state.stats.sessions = 5;
        app.state.stats.streak = 4;
        app.state.totals.minutes = 940;
        app.state.xp = 60;
        app.history.push("stats".into());
        app.transcript.push(Entry::You(
            "start a 25m focus block and drop \"Ship the landing page\" into Today".into(),
        ));
        app.transcript.push(Entry::Tool(
            crate::agent::ToolCall::new("pomocard.focus")
                .kv("duration", "25m")
                .kv("result", "25:00 on the clock"),
        ));
        app.transcript.push(Entry::Agent(app.agent_summary()));
        app
    }

    fn render(app: &App, w: u16, h: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).expect("backend");
        terminal.draw(|f| draw(f, app)).expect("draw");
        let buf = terminal.backend().buffer().clone();
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                out.push_str(buf.cell((x, y)).map(|c| c.symbol()).unwrap_or(" "));
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn renders_every_view() {
        let mut app = test_app();
        for view in View::ALL {
            app.view = view;
            let frame = render(&app, 140, 40);
            assert!(frame.contains("pomocard agent"), "chrome missing in {view:?}");
            println!("=== {} ===\n{frame}", view.title());
        }
    }

    #[test]
    fn renders_overlays_and_tiny_terminals() {
        let mut app = test_app();
        app.palette = Some(Palette {
            query: "board".into(),
            sel: 0,
        });
        let frame = render(&app, 100, 30);
        assert!(frame.contains("command palette"));
        app.palette = None;
        app.help = true;
        let frame = render(&app, 100, 30);
        assert!(frame.contains("keys & commands"));
        app.help = false;
        app.toast("Session complete", "+25 min focused");
        // pathologically small viewports must not panic
        for (w, h) in [(20u16, 8u16), (40, 12), (60, 16), (240, 60)] {
            let _ = render(&app, w, h);
        }
    }
}
