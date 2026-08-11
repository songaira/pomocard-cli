//! Individual views — the terminal translation of the web app's dashboard shell.

use chrono::{Datelike, Duration as ChronoDuration, Utc};
use ratatui::prelude::*;
use ratatui::symbols::Marker;
use ratatui::widgets::canvas::{Canvas, Points};
use ratatui::widgets::{Block, BorderType, Paragraph, Wrap};

use crate::app::{bool_label, App};
use crate::data::initials;
use crate::state::{col_label, fmt_minutes, tier_label, tier_price, COLS};
use crate::theme::{self, Theme};
use crate::ui::{transcript_lines, tier_note, truncate, wrapped_len};

fn panel<'a>(title: &'a str, th: &Theme) -> Block<'a> {
    let block = Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(th.border())
        .style(th.base());
    if title.is_empty() {
        block
    } else {
        block.title(Span::styled(format!(" {title} "), th.bold()))
    }
}

/// Monochrome bar: `████████░░░░  label`.
fn bar_line(label: &str, ratio: f64, value: String, width: usize, th: &Theme) -> Line<'static> {
    let track = width.max(4);
    let filled = ((ratio.clamp(0.0, 1.0)) * track as f64).round() as usize;
    Line::from(vec![
        Span::styled(format!(" {:<14}", truncate(label, 14)), th.dim()),
        Span::styled("█".repeat(filled), th.base()),
        Span::styled("░".repeat(track.saturating_sub(filled)), th.faint()),
        Span::styled(format!("  {value}"), th.dim()),
    ])
}

fn kv_line(key: &str, value: String, th: &Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!(" {:<16}", key), th.dim()),
        Span::styled(value, th.base()),
    ])
}

/* ---------------- agent ---------------- */

pub fn agent(f: &mut Frame, app: &App, area: Rect, th: &Theme) {
    let (left, right) = if area.width >= 96 {
        let cols = Layout::horizontal([Constraint::Min(40), Constraint::Length(34)]).split(area);
        (cols[0], Some(cols[1]))
    } else {
        (area, None)
    };

    let block = panel("transcript", th);
    let inner = block.inner(left);
    f.render_widget(block, left);

    let lines = transcript_lines(app, th);
    let total = wrapped_len(&lines, inner.width);
    let height = inner.height as usize;
    let max_scroll = total.saturating_sub(height) as u16;
    let offset = if app.follow {
        max_scroll
    } else {
        app.scroll.min(max_scroll)
    };
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((offset, 0)),
        inner,
    );

    if let Some(rail) = right {
        let rows = Layout::vertical([Constraint::Min(11), Constraint::Length(9)]).split(rail);
        timer_panel(f, app, rows[0], th);
        mini_board(f, app, rows[1], th);
    }
}

fn mini_board(f: &mut Frame, app: &App, area: Rect, th: &Theme) {
    let block = panel("board", th);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let mut lines: Vec<Line> = Vec::new();
    for col in COLS {
        let cards = app.state.board.column(col);
        let est: u32 = cards.iter().map(|c| c.est as u32).sum();
        lines.push(Line::from(vec![
            Span::styled(format!(" {:<12}", col_label(col)), th.dim()),
            Span::styled(
                format!("{:>2} card{}", cards.len(), if cards.len() == 1 { "" } else { "s" }),
                th.base(),
            ),
            Span::styled(format!("  ~{}", fmt_minutes(est * 25)), th.dim()),
        ]));
    }
    let task = app
        .state
        .active_card()
        .map(|c| c.title.clone())
        .unwrap_or_else(|| "No task selected".into());
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled(" current  ", th.dim()),
        Span::styled(truncate(&task, inner.width.saturating_sub(11) as usize), th.bold()),
    ]));
    f.render_widget(Paragraph::new(lines), inner);
}

/* ---------------- timer ---------------- */

pub fn timer_panel(f: &mut Frame, app: &App, area: Rect, th: &Theme) {
    let block = panel("pomodoro", th);
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.height < 6 {
        return;
    }

    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Max(13),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .split(inner);

    // mode tabs
    let mut spans = Vec::new();
    for mode in [
        crate::timer::Mode::Focus,
        crate::timer::Mode::Short,
        crate::timer::Mode::Long,
    ] {
        let style = if app.timer.mode == mode {
            th.inverse()
        } else {
            th.dim()
        };
        spans.push(Span::styled(format!(" {} ", mode.short_label()), style));
        spans.push(Span::raw(" "));
    }
    f.render_widget(Paragraph::new(Line::from(spans)).alignment(Alignment::Center), rows[0]);

    ring(f, app, rows[1], th);

    // controls
    let controls = Line::from(vec![
        Span::styled(
            if app.timer.running { " ‖ pause " } else { " ▶ start " },
            th.inverse(),
        ),
        Span::styled("  ↺ r ", th.dim()),
        Span::styled(" ⇥ n ", th.dim()),
    ]);
    f.render_widget(Paragraph::new(controls).alignment(Alignment::Center), rows[2]);

    // session dots
    let mut dots = vec![Span::styled(
        format!("Session {} / 4  ", app.state.session + 1),
        th.dim(),
    )];
    for i in 0..4 {
        let on = i < app.state.session;
        dots.push(Span::styled(
            format!("{} ", if on { theme::DOT_ON } else { theme::DOT_OFF }),
            if on { th.base() } else { th.faint() },
        ));
    }
    f.render_widget(Paragraph::new(Line::from(dots)).alignment(Alignment::Center), rows[3]);

    let stats = if inner.width < 34 {
        Line::from(vec![
            Span::styled(fmt_minutes(app.state.stats.minutes), th.base()),
            Span::styled(" · ", th.faint()),
            Span::styled(format!("{} sess", app.state.stats.sessions), th.base()),
            Span::styled(" · ", th.faint()),
            Span::styled(format!("{}d", app.state.stats.streak), th.base()),
        ])
    } else {
        Line::from(vec![
            Span::styled(format!("{} today", fmt_minutes(app.state.stats.minutes)), th.base()),
            Span::styled(" · ", th.faint()),
            Span::styled(format!("{} sessions", app.state.stats.sessions), th.base()),
            Span::styled(" · ", th.faint()),
            Span::styled(format!("{}d streak", app.state.stats.streak), th.base()),
        ])
    };
    f.render_widget(Paragraph::new(stats).alignment(Alignment::Center), rows[4]);

    // current task, like the web app's `.current-task` block
    if rows[5].height >= 2 {
        let task = app
            .state
            .active_card()
            .map(|c| c.title.clone())
            .unwrap_or_else(|| "No task selected".into());
        let est = app.state.active_card().map(|c| c.est).unwrap_or(0);
        let dots: String = (0..4).map(|i| if i < est { '●' } else { '○' }).collect();
        let mut lines = vec![
            Line::raw(""),
            Line::styled(" Current task".to_string(), th.dim()),
            Line::styled(
                format!(" {}", truncate(&task, rows[5].width.saturating_sub(2) as usize)),
                th.bold(),
            ),
        ];
        if est > 0 {
            lines.push(Line::styled(format!(" {dots}"), th.faint()));
        }
        lines.truncate(rows[5].height as usize);
        f.render_widget(Paragraph::new(lines), rows[5]);
    }
}

fn ring(f: &mut Frame, app: &App, area: Rect, th: &Theme) {
    let frac = app.timer.progress();
    let fg = th.fg;
    let line = th.line;
    let w = area.width.max(1) as f64;
    let h = (area.height.max(1) as f64) * 2.0;
    let aspect = (w / h).max(0.2);

    let canvas = Canvas::default()
        .marker(Marker::Braille)
        .x_bounds([-aspect, aspect])
        .y_bounds([-1.0, 1.0])
        .paint(move |ctx| {
            let r = 0.82;
            let steps = 360;
            let mut track: Vec<(f64, f64)> = Vec::new();
            let mut prog: Vec<(f64, f64)> = Vec::new();
            for i in 0..=steps {
                let t = i as f64 / steps as f64;
                let ang = std::f64::consts::FRAC_PI_2 - t * std::f64::consts::TAU;
                let p = (ang.cos() * r, ang.sin() * r);
                if t <= frac {
                    prog.push(p);
                } else {
                    track.push(p);
                }
            }
            ctx.draw(&Points {
                coords: &track,
                color: line,
            });
            ctx.draw(&Points {
                coords: &prog,
                color: fg,
            });
        });
    f.render_widget(canvas, area);

    // clock in the middle of the ring
    let label_w = 14u16.min(area.width);
    let cy = area.y + area.height / 2;
    let rect = Rect {
        x: area.x + (area.width.saturating_sub(label_w)) / 2,
        y: cy.saturating_sub(1),
        width: label_w,
        height: 2.min(area.height),
    };
    f.render_widget(
        Paragraph::new(vec![
            Line::styled(app.timer.clock(), th.bold()),
            Line::styled(app.timer.mode.label().to_string(), th.dim()),
        ])
        .alignment(Alignment::Center),
        rect,
    );
}

/* ---------------- board ---------------- */

pub fn board(f: &mut Frame, app: &App, area: Rect, th: &Theme) {
    let (timer_area, board_area) = if area.width >= 92 {
        let cols = Layout::horizontal([Constraint::Length(32), Constraint::Min(40)]).split(area);
        (Some(cols[0]), cols[1])
    } else {
        (None, area)
    };
    if let Some(t) = timer_area {
        timer_panel(f, app, t, th);
    }

    let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(3)]).split(board_area);
    let est = app.state.board.est_total();
    let toolbar = Line::from(vec![
        Span::styled(format!(" {} cards", app.state.board.total()), th.base()),
        Span::styled("  ·  ", th.faint()),
        Span::styled(format!("~{}", fmt_minutes(est * 25)), th.dim()),
        Span::styled("  ·  ", th.faint()),
        Span::styled("a add · x delete · c est · p pin · HJKL move", th.dim()),
    ]);
    f.render_widget(Paragraph::new(toolbar), rows[0]);

    let cols = Layout::horizontal([
        Constraint::Ratio(1, 4),
        Constraint::Ratio(1, 4),
        Constraint::Ratio(1, 4),
        Constraint::Ratio(1, 4),
    ])
    .split(rows[1]);

    for (ci, col) in COLS.iter().enumerate() {
        let cards = app.state.board.column(col);
        let selected_col = ci == app.sel_col;
        let title = format!("{} {}", col_label(col), cards.len());
        let mut block = panel("", th).title(Span::styled(
            format!(" {title} "),
            if selected_col { th.inverse() } else { th.bold() },
        ));
        if selected_col {
            block = block.border_style(th.base());
        }
        let inner = block.inner(cols[ci]);
        f.render_widget(block, cols[ci]);

        let mut lines: Vec<Line> = Vec::new();
        let width = inner.width.saturating_sub(2) as usize;
        for (ri, card) in cards.iter().enumerate() {
            let sel = selected_col && ri == app.sel_row;
            let marker = if app.state.active_id.as_deref() == Some(card.id.as_str()) {
                "▶ "
            } else if *col == "done" {
                "✓ "
            } else {
                "· "
            };
            let title_style = if sel {
                th.inverse()
            } else if *col == "done" {
                th.dim()
            } else {
                th.base()
            };
            lines.push(Line::from(vec![
                Span::styled(marker.to_string(), th.dim()),
                Span::styled(truncate(&card.title, width.saturating_sub(2)), title_style),
            ]));
            let dots: String = (0..4)
                .map(|i| if i < card.est { '●' } else { '○' })
                .collect();
            lines.push(Line::styled(format!("  {dots}"), th.faint()));
        }
        if cards.is_empty() {
            lines.push(Line::styled("  empty".to_string(), th.faint()));
        }
        f.render_widget(Paragraph::new(lines), inner);
    }
}

/* ---------------- analytics ---------------- */

pub fn analytics(f: &mut Frame, app: &App, area: Rect, th: &Theme) {
    let rows = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(12),
        Constraint::Min(6),
    ])
    .split(area);

    // KPIs
    let cells = Layout::horizontal([
        Constraint::Ratio(1, 4),
        Constraint::Ratio(1, 4),
        Constraint::Ratio(1, 4),
        Constraint::Ratio(1, 4),
    ])
    .split(rows[0]);
    let kpis = [
        (fmt_minutes(app.state.stats.minutes), "Today"),
        (app.state.stats.sessions.to_string(), "Sessions today"),
        (format!("{}d", app.state.stats.streak), "Streak"),
        (fmt_minutes(app.state.totals.minutes), "All-time focus"),
    ];
    for (i, (val, label)) in kpis.iter().enumerate() {
        let block = panel("", th);
        let inner = block.inner(cells[i]);
        f.render_widget(block, cells[i]);
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!(" {val}  "), th.bold()),
                Span::styled(label.to_string(), th.dim()),
            ])),
            inner,
        );
    }

    // heatmap + distribution
    let mid = Layout::horizontal([Constraint::Min(34), Constraint::Length(44)]).split(rows[1]);
    heatmap(f, app, mid[0], th);
    radar(f, app, mid[1], th);

    week_bars(f, app, rows[2], th);
}

fn heatmap(f: &mut Frame, app: &App, area: Rect, th: &Theme) {
    let block = panel("focus heatmap · last 14 weeks", th);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let today = Utc::now().date_naive();
    let monday = today - ChronoDuration::days(today.weekday().num_days_from_monday() as i64);
    let day_names = ["M", "T", "W", "T", "F", "S", "S"];
    let mut lines: Vec<Line> = Vec::new();
    for (wd, name) in day_names.iter().enumerate() {
        let mut spans = vec![Span::styled(format!(" {name} "), th.faint())];
        for week in 0..14i64 {
            let d = monday - ChronoDuration::weeks(13 - week) + ChronoDuration::days(wd as i64);
            if d > today {
                spans.push(Span::styled("  ".to_string(), th.base()));
                continue;
            }
            let mins = app.state.minutes_on(&d.format("%Y-%m-%d").to_string());
            let idx = match mins {
                0 => 0,
                1..=24 => 1,
                25..=49 => 2,
                50..=89 => 3,
                _ => 4,
            };
            let style = if idx == 0 { th.faint() } else { th.base() };
            spans.push(Span::styled(format!("{} ", theme::HEAT[idx]), style));
        }
        lines.push(Line::from(spans));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled(" less ", th.faint()),
        Span::styled(theme::HEAT.join(" "), th.dim()),
        Span::styled(" more", th.faint()),
    ]));
    f.render_widget(Paragraph::new(lines), inner);
}

fn radar(f: &mut Frame, app: &App, area: Rect, th: &Theme) {
    let block = panel("focus distribution", th);
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Deep work is real (today's sessions); the rest mirrors the web demo split.
    let deep = if app.state.stats.sessions > 0 { 0.85 } else { 0.55 };
    let cats: [(&str, f64); 6] = [
        ("Deep work", deep),
        ("Create", 0.70),
        ("Learn", 0.60),
        ("Review", 0.55),
        ("Meetings", 0.50),
        ("Admin", 0.40),
    ];
    let width = inner.width.saturating_sub(24) as usize;
    let lines: Vec<Line> = cats
        .iter()
        .map(|(name, v)| bar_line(name, *v, format!("{:>3}%", (v * 100.0) as u32), width, th))
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}

fn week_bars(f: &mut Frame, app: &App, area: Rect, th: &Theme) {
    let block = panel("this week · focus minutes per day", th);
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.height < 3 {
        return;
    }
    let today = Utc::now().date_naive();
    let mut values: Vec<(String, u32)> = Vec::new();
    for i in (0..7).rev() {
        let d = today - ChronoDuration::days(i);
        let label = match d.weekday().num_days_from_monday() {
            0 => "M",
            1 => "T",
            2 => "W",
            3 => "T",
            4 => "F",
            5 => "S",
            _ => "S",
        };
        values.push((
            label.to_string(),
            app.state.minutes_on(&d.format("%Y-%m-%d").to_string()),
        ));
    }
    let max = values.iter().map(|v| v.1).max().unwrap_or(0).max(25);
    let chart_h = inner.height.saturating_sub(2) as usize;
    let cell_w = (inner.width as usize / 7).max(3);

    let mut lines: Vec<Line> = Vec::new();
    for row in 0..chart_h {
        let mut spans: Vec<Span> = Vec::new();
        for (_, v) in &values {
            let bar_h = ((*v as f64 / max as f64) * chart_h as f64).round() as usize;
            let filled = bar_h + row >= chart_h;
            let glyph = if filled { "█" } else { " " };
            spans.push(Span::styled(
                format!("{:^width$}", glyph, width = cell_w),
                if filled { th.base() } else { th.faint() },
            ));
        }
        lines.push(Line::from(spans));
    }
    let mut labels: Vec<Span> = Vec::new();
    let mut nums: Vec<Span> = Vec::new();
    for (label, v) in &values {
        labels.push(Span::styled(
            format!("{:^width$}", label, width = cell_w),
            th.dim(),
        ));
        nums.push(Span::styled(
            format!("{:^width$}", v, width = cell_w),
            th.faint(),
        ));
    }
    lines.push(Line::from(labels));
    lines.push(Line::from(nums));
    f.render_widget(Paragraph::new(lines), inner);
}

/* ---------------- coach ---------------- */

pub fn coach(f: &mut Frame, app: &App, area: Rect, th: &Theme) {
    let cols = Layout::horizontal([Constraint::Min(36), Constraint::Length(40)]).split(area);

    let block = panel("AI focus coach", th);
    let inner = block.inner(cols[0]);
    f.render_widget(block, cols[0]);
    let mut lines: Vec<Line> = Vec::new();
    for (me, text) in &app.coach_feed {
        let who = if *me { "You" } else { "AI " };
        let style = if *me { th.base() } else { th.dim() };
        lines.push(Line::from(vec![
            Span::styled(format!(" {who}  "), th.bold()),
            Span::styled(text.clone(), style),
        ]));
        lines.push(Line::raw(""));
    }
    lines.push(Line::styled(
        " Ask anything in the prompt below — unmatched lines go to the coach.".to_string(),
        th.faint(),
    ));
    let total = wrapped_len(&lines, inner.width);
    let scroll = total.saturating_sub(inner.height as usize) as u16;
    f.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).scroll((scroll, 0)),
        inner,
    );

    let block = panel("insights", th);
    let inner = block.inner(cols[1]);
    f.render_widget(block, cols[1]);
    let mut lines: Vec<Line> = Vec::new();
    for insight in &app.data.insights.insights {
        lines.push(Line::from(vec![
            Span::styled(" ◆ ", th.dim()),
            Span::styled(insight.clone(), th.base()),
        ]));
        lines.push(Line::raw(""));
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

/* ---------------- templates ---------------- */

pub fn templates(f: &mut Frame, app: &App, area: Rect, th: &Theme) {
    let cols = Layout::horizontal([Constraint::Length(38), Constraint::Min(30)]).split(area);

    let block = panel("routines & templates", th);
    let inner = block.inner(cols[0]);
    f.render_widget(block, cols[0]);
    let mut lines: Vec<Line> = Vec::new();
    for (i, t) in app.data.templates.iter().enumerate() {
        let sel = i == app.tpl_sel;
        lines.push(Line::styled(
            format!(" {} ", t.name),
            if sel { th.inverse() } else { th.base() },
        ));
        lines.push(Line::styled(
            format!("   {} cards · 4 columns", t.cards.len()),
            th.faint(),
        ));
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        " j/k select · Enter loads into the board".to_string(),
        th.dim(),
    ));
    f.render_widget(Paragraph::new(lines), inner);

    let block = panel("preview", th);
    let inner = block.inner(cols[1]);
    f.render_widget(block, cols[1]);
    let mut lines: Vec<Line> = Vec::new();
    if let Some(t) = app.data.templates.get(app.tpl_sel) {
        lines.push(Line::styled(format!(" {}", t.name), th.bold()));
        lines.push(Line::styled(format!(" {}", t.desc), th.dim()));
        lines.push(Line::raw(""));
        for col in COLS {
            let cards: Vec<&crate::data::TemplateCard> =
                t.cards.iter().filter(|c| c.col == col).collect();
            if cards.is_empty() {
                continue;
            }
            lines.push(Line::styled(format!(" {}", col_label(col)), th.bold()));
            for c in cards {
                let dots: String = (0..4).map(|i| if i < c.est { '●' } else { '○' }).collect();
                lines.push(Line::from(vec![
                    Span::styled("   · ", th.faint()),
                    Span::styled(c.title.clone(), th.base()),
                    Span::styled(format!("  {dots}"), th.faint()),
                ]));
            }
            lines.push(Line::raw(""));
        }
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

/* ---------------- team ---------------- */

pub fn team(f: &mut Frame, app: &App, area: Rect, th: &Theme) {
    let rows = Layout::vertical([Constraint::Min(8), Constraint::Length(8)]).split(area);
    let cols = Layout::horizontal([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)]).split(rows[0]);

    let block = panel("members & roles", th);
    let inner = block.inner(cols[0]);
    f.render_widget(block, cols[0]);
    let lines: Vec<Line> = app
        .data
        .team
        .members
        .iter()
        .map(|m| {
            let ini = if m.initials.is_empty() {
                initials(&m.name)
            } else {
                m.initials.clone()
            };
            Line::from(vec![
                Span::styled(format!(" {ini} "), th.inverse()),
                Span::styled(format!(" {:<16}", m.name), th.base()),
                Span::styled(format!("{:<8}", m.role), th.dim()),
                Span::styled(
                    if m.online { "● online" } else { "○ offline" }.to_string(),
                    if m.online { th.base() } else { th.faint() },
                ),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);

    let block = panel("activity", th);
    let inner = block.inner(cols[1]);
    f.render_widget(block, cols[1]);
    let mut lines: Vec<Line> = Vec::new();
    for a in &app.data.team.activity {
        let to = if a.to.is_empty() {
            String::new()
        } else {
            format!(" → {}", a.to)
        };
        lines.push(Line::from(vec![
            Span::styled(format!(" {} ", initials(&a.who)), th.inverse()),
            Span::styled(format!(" {} {} {}{}", a.who, a.action, a.target, to), th.base()),
        ]));
        lines.push(Line::styled(format!("      {}", a.ago), th.faint()));
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);

    let block = panel("workload", th);
    let inner = block.inner(rows[1]);
    f.render_widget(block, rows[1]);
    let max = app
        .data
        .team
        .workload
        .iter()
        .map(|w| w.load)
        .max()
        .unwrap_or(1)
        .max(1);
    let width = inner.width.saturating_sub(28) as usize;
    let lines: Vec<Line> = app
        .data
        .team
        .workload
        .iter()
        .map(|w| {
            bar_line(
                &w.name,
                w.load as f64 / max as f64,
                format!("{} cards", w.load),
                width,
                th,
            )
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}

/* ---------------- billing ---------------- */

pub fn billing(f: &mut Frame, app: &App, area: Rect, th: &Theme) {
    let block = panel("billing & seats · demo, no charges", th);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let mut lines: Vec<Line> = Vec::new();
    for m in &app.data.team.members {
        let ini = if m.initials.is_empty() {
            initials(&m.name)
        } else {
            m.initials.clone()
        };
        lines.push(Line::from(vec![
            Span::styled(format!(" {ini} "), th.inverse()),
            Span::styled(format!(" {:<20}", m.name), th.base()),
            Span::styled(format!("{:<10}", m.role), th.dim()),
            Span::styled("$12/mo".to_string(), th.base()),
        ]));
    }
    let seats = app.data.team.members.len();
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled(format!(" {seats} seats × $12"), th.dim()),
        Span::styled(format!("   ${}/mo", seats * 12), th.bold()),
    ]));
    lines.push(Line::styled(
        " Billed monthly · cancel anytime · this build never calls a payment API.".to_string(),
        th.faint(),
    ));
    f.render_widget(Paragraph::new(lines), inner);
}

/* ---------------- settings ---------------- */

pub fn settings(f: &mut Frame, app: &App, area: Rect, th: &Theme) {
    let cols = Layout::horizontal([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)]).split(area);
    let left = Layout::vertical([Constraint::Length(10), Constraint::Min(6)]).split(cols[0]);

    let block = panel("timer settings", th);
    let inner = block.inner(left[0]);
    f.render_widget(block, left[0]);
    let s = &app.state.settings;
    let lines = vec![
        kv_line("focus", format!("{} min", s.focus), th),
        kv_line("short break", format!("{} min", s.short), th),
        kv_line("long break", format!("{} min", s.long), th),
        kv_line("auto-start", bool_label(s.auto).to_string(), th),
        kv_line("chime", format!("{} ({})", s.sound_theme, bool_label(s.sound)), th),
        kv_line("theme", app.state.theme.clone(), th),
        Line::styled(
            " set focus 50 · set auto on · set chime pad".to_string(),
            th.faint(),
        ),
    ];
    f.render_widget(Paragraph::new(lines), inner);

    let block = panel("account, plan & ambient", th);
    let inner = block.inner(left[1]);
    f.render_widget(block, left[1]);
    let account = app
        .state
        .account
        .as_ref()
        .map(|a| format!("{} <{}>", a.name, a.email))
        .unwrap_or_else(|| "not signed in — data stays local".into());
    let mut lines = vec![
        kv_line("account", account, th),
        kv_line(
            "plan",
            format!(
                "{} ({})",
                tier_label(&app.state.tier),
                tier_price(&app.state.tier)
            ),
            th,
        ),
        kv_line("state file", app.path.display().to_string(), th),
        kv_line(
            "ambient",
            app.state
                .ambient
                .clone()
                .unwrap_or_else(|| "off".into()),
            th,
        ),
        Line::styled(
            " ambient synthesis (noise · rain · binaural) runs in the browser build;".to_string(),
            th.faint(),
        ),
        Line::styled(
            " the CLI stores your choice and rings the terminal bell on completion.".to_string(),
            th.faint(),
        ),
        Line::raw(""),
        Line::styled(" u cycles the demo plan · sync writes the JSON now".to_string(), th.dim()),
    ];
    lines.truncate(inner.height as usize);
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);

    let block = panel("achievements & xp", th);
    let inner = block.inner(cols[1]);
    f.render_widget(block, cols[1]);
    let rows = Layout::vertical([Constraint::Length(2), Constraint::Min(3)]).split(inner);
    let xp = app.state.xp % 100;
    f.render_widget(
        Paragraph::new(bar_line(
            &format!("Level {}", app.level()),
            xp as f64 / 100.0,
            format!("{} xp", app.state.xp),
            inner.width.saturating_sub(28) as usize,
            th,
        )),
        rows[0],
    );
    let lines: Vec<Line> = app
        .achievement_rows()
        .iter()
        .map(|a| {
            Line::from(vec![
                Span::styled(
                    format!(" {} ", if a.unlocked { theme::STAR } else { "·" }),
                    if a.unlocked { th.base() } else { th.faint() },
                ),
                Span::styled(
                    format!("{:<16}", a.name),
                    if a.unlocked { th.bold() } else { th.faint() },
                ),
                Span::styled(a.desc.to_string(), th.dim()),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(lines), rows[1]);
}

/* ---------------- paywall ---------------- */

pub fn locked(f: &mut Frame, app: &App, area: Rect, th: &Theme, need: &str) {
    let block = panel(app.view.title(), th);
    let inner = block.inner(area);
    f.render_widget(block, area);

    // "blurred" skeleton of the real surface
    let mut lines: Vec<Line> = Vec::new();
    for i in 0..inner.height {
        let width = inner.width.saturating_sub(4) as usize;
        let fill = match i % 4 {
            0 => width * 3 / 4,
            1 => width / 2,
            2 => width * 5 / 8,
            _ => width / 3,
        };
        lines.push(Line::styled(
            format!("  {}", theme::LOCK.repeat(fill.max(1))),
            th.faint(),
        ));
    }
    f.render_widget(Paragraph::new(lines), inner);

    let rect = crate::ui::centered(inner, 56, 7);
    f.render_widget(ratatui::widgets::Clear, rect);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(th.border())
        .style(th.base());
    let lock_inner = block.inner(rect);
    f.render_widget(block, rect);
    f.render_widget(
        Paragraph::new(vec![
            Line::styled(format!("{} {} feature", theme::LOCK, tier_label(need)), th.bold()),
            Line::raw(""),
            Line::styled(tier_note(need), th.dim()),
            Line::styled(format!("type: upgrade {need}"), th.base()),
        ])
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true }),
        lock_inner,
    );
}
