//! Application state machine shared by the TUI and the headless `exec` mode.

use std::path::PathBuf;
use std::time::Instant;

use anyhow::Result;

use crate::agent::{self, Cmd, Entry, ToolCall};
use crate::data::Data;
use crate::state::{
    col_label, fmt_minutes, normalize_col, tier_label, tier_price, Account, Achievement, State,
    COLS,
};
use crate::timer::{Mode, Timer};

/* ---------------- views ---------------- */

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Agent,
    Board,
    Analytics,
    Coach,
    Templates,
    Team,
    Billing,
    Settings,
}

impl View {
    pub const ALL: [View; 8] = [
        View::Agent,
        View::Board,
        View::Analytics,
        View::Coach,
        View::Templates,
        View::Team,
        View::Billing,
        View::Settings,
    ];

    pub fn title(self) -> &'static str {
        match self {
            View::Agent => "Agent",
            View::Board => "Board",
            View::Analytics => "Analytics",
            View::Coach => "AI Coach",
            View::Templates => "Templates",
            View::Team => "Team",
            View::Billing => "Billing",
            View::Settings => "Settings",
        }
    }

    /// Tier required, mirroring VIEW_TIER in plus.js.
    pub fn tier(self) -> Option<&'static str> {
        match self {
            View::Analytics | View::Coach | View::Templates => Some("pro"),
            View::Team | View::Billing => Some("team"),
            _ => None,
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            View::Agent => "agent",
            View::Board => "board",
            View::Analytics => "analytics",
            View::Coach => "coach",
            View::Templates => "templates",
            View::Team => "team",
            View::Billing => "billing",
            View::Settings => "settings",
        }
    }

    pub fn from_slug(s: &str) -> Option<View> {
        View::ALL.into_iter().find(|v| v.slug() == s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Prompt,
}

pub struct Toast {
    pub title: String,
    pub sub: String,
    pub at: Instant,
}

pub struct Palette {
    pub query: String,
    pub sel: usize,
}

#[derive(Debug, Clone)]
pub struct PaletteAction {
    pub label: String,
    pub hint: String,
    pub cmd: String,
}

/* ---------------- achievements (ACHS in plus.js) ---------------- */

struct Ach {
    id: &'static str,
    name: &'static str,
    desc: &'static str,
}

const ACHS: [Ach; 7] = [
    Ach { id: "first", name: "First focus", desc: "Complete a session" },
    Ach { id: "streak3", name: "On a roll", desc: "3-day streak" },
    Ach { id: "ten", name: "In the zone", desc: "10 sessions" },
    Ach { id: "cycle", name: "Full cycle", desc: "4 sessions in a row" },
    Ach { id: "pro", name: "Pro", desc: "Upgraded to Pro" },
    Ach { id: "team", name: "Team player", desc: "Joined a team" },
    Ach { id: "tpl", name: "Template", desc: "Loaded a routine" },
];

pub struct AchievementRow {
    pub name: &'static str,
    pub desc: &'static str,
    pub unlocked: bool,
}

/* ---------------- app ---------------- */

pub struct App {
    pub state: State,
    pub path: PathBuf,
    pub data: Data,
    pub timer: Timer,
    pub view: View,
    pub mode: InputMode,
    pub input: String,
    pub cursor: usize,
    pub history: Vec<String>,
    pub hist_idx: Option<usize>,
    pub transcript: Vec<Entry>,
    pub scroll: u16,
    pub follow: bool,
    pub sel_col: usize,
    pub sel_row: usize,
    pub tpl_sel: usize,
    pub coach_feed: Vec<(bool, String)>,
    pub palette: Option<Palette>,
    pub toast: Option<Toast>,
    pub help: bool,
    pub should_quit: bool,
    pub session_id: String,
    pub cwd: String,
    pub bell: bool,
    pub cycle_done: bool,
    pub used_template: bool,
    pub tick_count: u64,
    /// `true` for one-shot CLI runs, where no clock is actually ticking.
    pub headless: bool,
}

impl App {
    pub fn new(path: PathBuf, data: Data) -> Result<App> {
        let mut state = State::load(&path)?;
        if state.settings.openrouter_key.is_none() {
            if let Ok(k) = std::env::var("OPENROUTER_API_KEY") {
                if !k.trim().is_empty() {
                    state.settings.openrouter_key = Some(k);
                }
            }
        }
        let timer = Timer::new(&state.settings);
        let session_id = short_session_id();
        let cwd = pretty_cwd();
        let coach_line = data.coach_line(0);

        let mut app = App {
            state,
            path,
            data,
            timer,
            view: View::Agent,
            mode: InputMode::Prompt,
            input: String::new(),
            cursor: 0,
            history: Vec::new(),
            hist_idx: None,
            transcript: Vec::new(),
            scroll: 0,
            follow: true,
            sel_col: 1,
            sel_row: 0,
            tpl_sel: 0,
            coach_feed: vec![(false, coach_line)],
            palette: None,
            toast: None,
            help: false,
            should_quit: false,
            session_id,
            cwd,
            bell: false,
            cycle_done: false,
            used_template: false,
            tick_count: 0,
            headless: false,
        };
        app.transcript.push(Entry::Agent(format!(
            "Pomocard agent ready. {} · type a command, or press ? for keys.",
            app.headline()
        )));
        app.clamp_selection();
        Ok(app)
    }

    pub fn save(&self) {
        if let Err(err) = self.state.save(&self.path) {
            eprintln!("pomocard: could not save state: {err:#}");
        }
    }

    pub fn toast(&mut self, title: impl Into<String>, sub: impl Into<String>) {
        self.toast = Some(Toast {
            title: title.into(),
            sub: sub.into(),
            at: Instant::now(),
        });
    }

    /* ---------------- summaries ---------------- */

    pub fn headline(&self) -> String {
        let doing = self.state.board.doing.len();
        let done = self.state.board.done.len();
        let total = self.state.board.total();
        format!(
            "{} active · {} focused today · {}d streak · {}/{} cards",
            doing,
            fmt_minutes(self.state.stats.minutes),
            self.state.stats.streak,
            done,
            total
        )
    }

    pub fn agent_summary(&self) -> String {
        let tail = if self.timer.running && !self.headless {
            format!("{} left on the clock.", self.timer.clock())
        } else {
            "Queue the next block?".to_string()
        };
        format!("{}. {}", self.headline(), tail)
    }

    /* ---------------- timer plumbing ---------------- */

    pub fn tick(&mut self) {
        self.tick_count += 1;
        if self.timer.tick() {
            self.on_session_complete();
        }
        if let Some(t) = &self.toast {
            if t.at.elapsed().as_secs_f32() > 3.6 {
                self.toast = None;
            }
        }
    }

    fn on_session_complete(&mut self) {
        let was_focus = self.timer.mode == Mode::Focus;
        if self.state.settings.sound && self.state.settings.sound_theme != "none" {
            self.bell = true;
        }
        if was_focus {
            let minutes = (self.timer.total / 60.0).round() as u32;
            self.state.record_focus_session(minutes.max(1));
            self.state.session = (self.state.session + 1) % 4;
            if self.state.session == 0 {
                self.cycle_done = true;
            }
            let next = if self.state.session == 0 {
                Mode::Long
            } else {
                Mode::Short
            };
            self.timer.set_mode(&self.state.settings, next);
            self.transcript.push(Entry::Note(format!(
                "Focus session complete · +{} min · {} up next",
                minutes,
                next.label()
            )));
            self.toast("Session complete", format!("+{minutes} min focused"));
        } else {
            self.timer.set_mode(&self.state.settings, Mode::Focus);
            self.transcript
                .push(Entry::Note("Break over — back to focus.".to_string()));
            self.toast("Break over", "Focus is queued");
        }
        self.award();
        if self.state.settings.auto {
            self.timer.start();
        }
        self.follow = true;
        self.save();
    }

    /* ---------------- achievements ---------------- */

    pub fn award(&mut self) {
        let mut unlocked: Vec<&'static str> = Vec::new();
        for ach in ACHS.iter() {
            if self.state.achievements.iter().any(|a| a.id == ach.id) {
                continue;
            }
            let got = match ach.id {
                "first" => self.state.totals.sessions >= 1,
                "streak3" => self.state.stats.streak >= 3,
                "ten" => self.state.totals.sessions >= 10,
                "cycle" => self.cycle_done,
                "pro" => self.state.tier != "free",
                "team" => self.state.tier == "team",
                "tpl" => self.used_template,
                _ => false,
            };
            if got {
                self.state.achievements.push(Achievement {
                    id: ach.id.to_string(),
                    name: ach.name.to_string(),
                });
                self.state.xp += 25;
                unlocked.push(ach.name);
            }
        }
        for name in unlocked {
            self.transcript
                .push(Entry::Note(format!("★ Achievement unlocked — {name}")));
            self.toast("Achievement unlocked", name);
        }
    }

    pub fn level(&self) -> u32 {
        self.state.xp / 100 + 1
    }

    pub fn achievement_rows(&self) -> Vec<AchievementRow> {
        ACHS.iter()
            .map(|a| AchievementRow {
                name: a.name,
                desc: a.desc,
                unlocked: self.state.achievements.iter().any(|x| x.id == a.id),
            })
            .collect()
    }

    /* ---------------- navigation ---------------- */

    pub fn go(&mut self, view: View) {
        if let Some(need) = view.tier() {
            if !self.state.tier_ok(need) {
                self.view = view; // render the locked overlay, like the web paywall
                self.toast(
                    format!("{} feature", tier_label(need)),
                    format!("Upgrade to {} ({})", tier_label(need), tier_price(need)),
                );
                return;
            }
        }
        self.view = view;
    }

    pub fn locked(&self, view: View) -> Option<&'static str> {
        match view.tier() {
            Some(need) if !self.state.tier_ok(need) => Some(need),
            _ => None,
        }
    }

    pub fn cycle_view(&mut self, delta: i32) {
        let idx = View::ALL.iter().position(|v| *v == self.view).unwrap_or(0) as i32;
        let len = View::ALL.len() as i32;
        let next = ((idx + delta) % len + len) % len;
        self.go(View::ALL[next as usize]);
    }

    pub fn clamp_selection(&mut self) {
        self.sel_col = self.sel_col.min(COLS.len() - 1);
        let len = self.state.board.column(COLS[self.sel_col]).len();
        if len == 0 {
            self.sel_row = 0;
        } else {
            self.sel_row = self.sel_row.min(len - 1);
        }
    }

    pub fn selected_card_id(&self) -> Option<String> {
        self.state
            .board
            .column(COLS[self.sel_col])
            .get(self.sel_row)
            .map(|c| c.id.clone())
    }

    /* ---------------- palette ---------------- */

    pub fn palette_actions(&self) -> Vec<PaletteAction> {
        let mut out = vec![
            PaletteAction { label: "Start / pause timer".into(), hint: "space".into(), cmd: "start".into() },
            PaletteAction { label: "Reset timer".into(), hint: "r".into(), cmd: "reset".into() },
            PaletteAction { label: "Skip to next session".into(), hint: "n".into(), cmd: "skip".into() },
            PaletteAction { label: "Go to Board".into(), hint: "2".into(), cmd: "open board".into() },
            PaletteAction { label: "Go to Analytics".into(), hint: "pro".into(), cmd: "open analytics".into() },
            PaletteAction { label: "Go to AI Coach".into(), hint: "pro".into(), cmd: "open coach".into() },
            PaletteAction { label: "Go to Templates".into(), hint: "pro".into(), cmd: "open templates".into() },
            PaletteAction { label: "Go to Team console".into(), hint: "team".into(), cmd: "open team".into() },
            PaletteAction { label: "Go to Billing".into(), hint: "team".into(), cmd: "open billing".into() },
            PaletteAction { label: "Go to Settings".into(), hint: "8".into(), cmd: "open settings".into() },
            PaletteAction { label: "Toggle dark mode".into(), hint: "T".into(), cmd: "theme toggle".into() },
            PaletteAction { label: "Clear the Done column".into(), hint: "".into(), cmd: "clear done".into() },
            PaletteAction { label: "Upgrade to Pro".into(), hint: "$6/mo".into(), cmd: "upgrade pro".into() },
            PaletteAction { label: "Upgrade to Team".into(), hint: "$12/user".into(), cmd: "upgrade team".into() },
            PaletteAction { label: "Sync now".into(), hint: "".into(), cmd: "sync".into() },
        ];
        for col in COLS {
            for card in self.state.board.column(col) {
                out.push(PaletteAction {
                    label: format!("Focus: {}", card.title),
                    hint: col_label(col).to_string(),
                    cmd: format!("pin \"{}\"", card.title),
                });
            }
        }
        out
    }

    pub fn filtered_palette(&self) -> Vec<PaletteAction> {
        let Some(p) = &self.palette else {
            return Vec::new();
        };
        let q = p.query.trim().to_lowercase();
        let all = self.palette_actions();
        if q.is_empty() {
            return all;
        }
        all.into_iter()
            .filter(|a| {
                a.label.to_lowercase().contains(&q) || a.hint.to_lowercase().contains(&q)
            })
            .collect()
    }

    /* ---------------- command execution ---------------- */

    /// Runs a whole prompt line: `You` turn, tool calls, `Agent` summary.
    pub fn exec_line(&mut self, line: &str) -> Vec<Entry> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return Vec::new();
        }
        let mut produced: Vec<Entry> = Vec::new();
        produced.push(Entry::You(trimmed.to_string()));

        let cmds = self.resolve_cmds(trimmed);
        let mut asked = false;
        for cmd in cmds {
            if matches!(cmd, Cmd::Ask { .. }) {
                asked = true;
            }
            produced.extend(self.exec_cmd(cmd));
        }
        if !asked {
            produced.push(Entry::Agent(self.agent_summary()));
        }

        self.transcript.extend(produced.clone());
        self.follow = true;
        self.award();
        self.save();
        produced
    }

    /// Resolve a prompt into commands. With an OpenRouter key set, a `:free`
    /// model translates the request into local commands; otherwise we fall back
    /// to the built-in rule-based parser.
    fn resolve_cmds(&self, line: &str) -> Vec<Cmd> {
        if let Some(key) = &self.state.settings.openrouter_key {
            if !key.trim().is_empty() {
                if let Ok(text) = crate::llm::complete(key, &self.state.settings.model, line) {
                    let mut cmds = Vec::new();
                    for l in text.lines() {
                        let l = l.trim();
                        if !l.is_empty() {
                            cmds.extend(agent::parse(l));
                        }
                    }
                    if !cmds.is_empty() {
                        return cmds;
                    }
                }
            }
        }
        agent::parse(line)
    }

    pub fn exec_cmd(&mut self, cmd: Cmd) -> Vec<Entry> {
        let mut out: Vec<Entry> = Vec::new();
        match cmd {
            Cmd::Start { minutes } => {
                if let Some(m) = minutes {
                    self.timer.set_mode(&self.state.settings, Mode::Focus);
                    self.timer.set_duration(m);
                }
                self.timer.start();
                let mut tool = ToolCall::new("pomocard.focus")
                    .kv("mode", self.timer.mode.short_label().to_lowercase())
                    .kv("duration", format!("{}m", (self.timer.total / 60.0).round()))
                    .kv("result", format!("{} on the clock", self.timer.clock()));
                if self.headless {
                    tool = tool.kv("note", "the clock only ticks inside the TUI — run `pomocard`");
                }
                out.push(Entry::Tool(tool));
            }
            Cmd::Pause => {
                self.timer.pause();
                out.push(Entry::Tool(
                    ToolCall::new("pomocard.pause")
                        .kv("result", format!("paused at {}", self.timer.clock())),
                ));
            }
            Cmd::Reset => {
                self.timer.reset();
                out.push(Entry::Tool(
                    ToolCall::new("pomocard.reset")
                        .kv("mode", self.timer.mode.short_label().to_lowercase())
                        .kv("result", format!("back to {}", self.timer.clock())),
                ));
            }
            Cmd::Skip => {
                let next = match self.timer.mode {
                    Mode::Focus => {
                        if self.state.session >= 3 {
                            Mode::Long
                        } else {
                            Mode::Short
                        }
                    }
                    _ => Mode::Focus,
                };
                self.timer.set_mode(&self.state.settings, next);
                out.push(Entry::Tool(
                    ToolCall::new("pomocard.skip")
                        .kv("next", next.label())
                        .kv("result", format!("{} queued", self.timer.clock())),
                ));
            }
            Cmd::SetMode { mode, minutes } => {
                let m = Mode::parse(&mode).unwrap_or(Mode::Short);
                self.timer.set_mode(&self.state.settings, m);
                if let Some(mm) = minutes {
                    self.timer.set_duration(mm);
                }
                out.push(Entry::Tool(
                    ToolCall::new("pomocard.mode")
                        .kv("mode", m.label())
                        .kv("result", format!("{} on the clock", self.timer.clock())),
                ));
            }
            Cmd::Add { title, col, est } => {
                let col = normalize_col(&col).unwrap_or("today");
                if title.trim().is_empty() {
                    out.push(Entry::Tool(
                        ToolCall::new("pomocard.card.add")
                            .kv("error", "no title — try: add \"Ship the landing page\" to today")
                            .failed(),
                    ));
                } else {
                    self.state.add_card(col, &title, est);
                    let n = self.state.board.column(col).len();
                    out.push(Entry::Tool(
                        ToolCall::new("pomocard.card.add")
                            .kv("column", col)
                            .kv("title", format!("\"{title}\""))
                            .kv("est", format!("{est} pom")),
                    ));
                    out.push(Entry::Tool(
                        ToolCall::new("pomocard.board.sync")
                            .kv("result", format!("added → {} ({} cards)", col_label(col), n)),
                    ));
                }
            }
            Cmd::Move { query, col } => {
                let col = normalize_col(&col).unwrap_or("today");
                match self.state.board.search(&query) {
                    Some((from, i)) => {
                        let card = self.state.board.column(from)[i].clone();
                        self.state.move_card(&card.id, col, None);
                        out.push(Entry::Tool(
                            ToolCall::new("pomocard.card.move")
                                .kv("card", format!("\"{}\"", card.title))
                                .kv("from", col_label(from))
                                .kv("to", col_label(col))
                                .kv("result", "moved"),
                        ));
                    }
                    None => out.push(Entry::Tool(
                        ToolCall::new("pomocard.card.move")
                            .kv("query", format!("\"{query}\""))
                            .kv("error", "no card matched")
                            .failed(),
                    )),
                }
            }
            Cmd::Rename { query, title } => match self.state.board.search(&query) {
                Some((col, i)) => {
                    let old = self.state.board.column(col)[i].title.clone();
                    self.state.board.column_mut(col)[i].title = title.clone();
                    out.push(Entry::Tool(
                        ToolCall::new("pomocard.card.rename")
                            .kv("from", format!("\"{old}\""))
                            .kv("to", format!("\"{title}\""))
                            .kv("result", "renamed"),
                    ));
                }
                None => out.push(Entry::Tool(
                    ToolCall::new("pomocard.card.rename")
                        .kv("query", format!("\"{query}\""))
                        .kv("error", "no card matched")
                        .failed(),
                )),
            },
            Cmd::Delete { query } => match self.state.board.search(&query) {
                Some((col, i)) => {
                    let id = self.state.board.column(col)[i].id.clone();
                    let removed = self.state.delete_card(&id);
                    let title = removed.map(|c| c.title).unwrap_or_default();
                    out.push(Entry::Tool(
                        ToolCall::new("pomocard.card.del")
                            .kv("card", format!("\"{title}\""))
                            .kv("column", col_label(col))
                            .kv("result", "deleted"),
                    ));
                }
                None => out.push(Entry::Tool(
                    ToolCall::new("pomocard.card.del")
                        .kv("query", format!("\"{query}\""))
                        .kv("error", "no card matched")
                        .failed(),
                )),
            },
            Cmd::Pin { query } => match self.state.board.search(&query) {
                Some((col, i)) => {
                    let card = self.state.board.column(col)[i].clone();
                    self.state.set_active(Some(&card.id));
                    out.push(Entry::Tool(
                        ToolCall::new("pomocard.card.pin")
                            .kv("card", format!("\"{}\"", card.title))
                            .kv("column", col_label(col))
                            .kv("result", "pinned as current task"),
                    ));
                }
                None => out.push(Entry::Tool(
                    ToolCall::new("pomocard.card.pin")
                        .kv("query", format!("\"{query}\""))
                        .kv("error", "no card matched")
                        .failed(),
                )),
            },
            Cmd::Unpin => {
                self.state.set_active(None);
                out.push(Entry::Tool(
                    ToolCall::new("pomocard.card.pin").kv("result", "cleared current task"),
                ));
            }
            Cmd::Est { query, est } => match self.state.board.search(&query) {
                Some((col, i)) => {
                    self.state.board.column_mut(col)[i].est = est.clamp(1, 4);
                    let title = self.state.board.column(col)[i].title.clone();
                    out.push(Entry::Tool(
                        ToolCall::new("pomocard.card.est")
                            .kv("card", format!("\"{title}\""))
                            .kv("est", format!("{est} pom"))
                            .kv("result", "updated"),
                    ));
                }
                None => out.push(Entry::Tool(
                    ToolCall::new("pomocard.card.est")
                        .kv("query", format!("\"{query}\""))
                        .kv("error", "no card matched")
                        .failed(),
                )),
            },
            Cmd::ClearDone => {
                let n = self.state.clear_done();
                out.push(Entry::Tool(
                    ToolCall::new("pomocard.board.clear")
                        .kv("column", "done")
                        .kv("result", format!("{n} cards archived")),
                ));
            }
            Cmd::ListBoard => {
                let mut tool = ToolCall::new("pomocard.board.list");
                for col in COLS {
                    let cards = self.state.board.column(col);
                    let titles = if cards.is_empty() {
                        "—".to_string()
                    } else {
                        cards
                            .iter()
                            .map(|c| c.title.as_str())
                            .collect::<Vec<_>>()
                            .join(" · ")
                    };
                    tool = tool.kv(col, format!("({}) {}", cards.len(), titles));
                }
                out.push(Entry::Tool(tool));
            }
            Cmd::Stats => {
                let s = &self.state.stats;
                out.push(Entry::Tool(
                    ToolCall::new("pomocard.stats")
                        .kv("today", fmt_minutes(s.minutes))
                        .kv("sessions", s.sessions.to_string())
                        .kv("streak", format!("{}d", s.streak))
                        .kv("all time", fmt_minutes(self.state.totals.minutes))
                        .kv("level", format!("{} ({} xp)", self.level(), self.state.xp))
                        .kv("plan", tier_label(&self.state.tier)),
                ));
            }
            Cmd::Template { name } => {
                if !self.state.tier_ok("pro") {
                    out.push(self.locked_tool("pomocard.template.load", "pro"));
                } else {
                    match self.data.find_template(&name) {
                        Some(t) => {
                            let t = t.clone();
                            for col in COLS {
                                self.state.board.column_mut(col).clear();
                            }
                            for card in &t.cards {
                                let col = normalize_col(&card.col).unwrap_or("backlog");
                                self.state.add_card(col, &card.title, card.est);
                            }
                            self.state.active_id = None;
                            self.used_template = true;
                            self.clamp_selection();
                            out.push(Entry::Tool(
                                ToolCall::new("pomocard.template.load")
                                    .kv("template", t.name.clone())
                                    .kv("cards", t.cards.len().to_string())
                                    .kv("result", "board seeded"),
                            ));
                        }
                        None => {
                            let names = self
                                .data
                                .templates
                                .iter()
                                .map(|t| t.name.as_str())
                                .collect::<Vec<_>>()
                                .join(" · ");
                            out.push(Entry::Tool(
                                ToolCall::new("pomocard.template.load")
                                    .kv("query", format!("\"{name}\""))
                                    .kv("available", names)
                                    .kv("error", "no template matched")
                                    .failed(),
                            ));
                        }
                    }
                }
            }
            Cmd::Upgrade { tier } => {
                let tier = match tier.as_str() {
                    "team" => "team",
                    "pro" => "pro",
                    _ => "free",
                };
                self.state.tier = tier.to_string();
                if tier != "free" {
                    let mut acc = self.state.account.clone().unwrap_or(Account {
                        name: "Focused Friend".into(),
                        email: "you@studio.com".into(),
                        plan: tier.to_string(),
                    });
                    acc.plan = tier.to_string();
                    self.state.account = Some(acc);
                }
                out.push(Entry::Tool(
                    ToolCall::new("pomocard.plan.set")
                        .kv("plan", tier_label(tier))
                        .kv("price", tier_price(tier))
                        .kv("result", "demo unlock — no payment taken"),
                ));
                self.toast(format!("{} unlocked", tier_label(tier)), "Demo mode");
            }
            Cmd::Theme { theme } => {
                let next = match theme.as_str() {
                    "dark" => "dark",
                    "light" => "light",
                    _ => {
                        if self.state.theme == "dark" {
                            "light"
                        } else {
                            "dark"
                        }
                    }
                };
                self.state.theme = next.to_string();
                out.push(Entry::Tool(
                    ToolCall::new("pomocard.theme").kv("theme", next).kv("result", "applied"),
                ));
            }
            Cmd::Set { key, value } => out.push(Entry::Tool(self.apply_setting(&key, &value))),
            Cmd::View { name } => match View::from_slug(&name) {
                Some(v) => {
                    let locked = self.locked(v);
                    self.go(v);
                    let mut tool = ToolCall::new("pomocard.view").kv("view", v.title());
                    if let Some(need) = locked {
                        tool = tool
                            .kv("locked", format!("{} required", tier_label(need)))
                            .failed();
                    } else {
                        tool = tool.kv("result", "opened");
                    }
                    out.push(Entry::Tool(tool));
                }
                None => out.push(Entry::Tool(
                    ToolCall::new("pomocard.view")
                        .kv("query", name)
                        .kv("error", "unknown view")
                        .failed(),
                )),
            },
            Cmd::Sync => {
                out.push(Entry::Tool(
                    ToolCall::new("pomocard.sync")
                        .kv("target", self.path.display().to_string())
                        .kv("account", self
                            .state
                            .account
                            .as_ref()
                            .map(|a| a.email.clone())
                            .unwrap_or_else(|| "local only".into()))
                        .kv("result", "all changes saved"),
                ));
            }
            Cmd::Export => {
                let json = serde_json::to_string(&self.state).unwrap_or_default();
                out.push(Entry::Tool(
                    ToolCall::new("pomocard.export")
                        .kv("bytes", json.len().to_string())
                        .kv("hint", "localStorage.setItem('pomocard.v2', <file contents>)")
                        .kv("file", self.path.display().to_string()),
                ));
            }
            Cmd::Help => {
                self.help = true;
                out.push(Entry::Tool(
                    ToolCall::new("pomocard.help")
                        .kv("keys", "? overlay · ^K palette · Tab views · space start")
                        .kv("try", "add \"Write the spec\" to today · finish \"…\" · stats"),
                ));
            }
            Cmd::Quit => {
                self.should_quit = true;
                out.push(Entry::Tool(ToolCall::new("pomocard.quit").kv("result", "state saved")));
            }
            Cmd::Ask { text } => {
                if !text.trim().is_empty() {
                    self.coach_feed.push((true, text.clone()));
                }
                let n = (self.coach_feed.len() + self.tick_count as usize) % 5;
                let reply = self.data.coach_line(n);
                let insight = self
                    .data
                    .insights
                    .insights
                    .get(n % self.data.insights.insights.len().max(1))
                    .cloned()
                    .unwrap_or_default();
                let full = if self.state.tier_ok("pro") {
                    format!("{reply} {insight}")
                } else {
                    format!("{reply} (AI Coach insights are a Pro feature — try `upgrade pro`.)")
                };
                self.coach_feed.push((false, full.clone()));
                out.push(Entry::Agent(full));
            }
        }
        out
    }

    fn locked_tool(&self, name: &str, need: &str) -> Entry {
        Entry::Tool(
            ToolCall::new(name)
                .kv("locked", format!("{} feature", tier_label(need)))
                .kv("price", tier_price(need))
                .kv("hint", format!("run: upgrade {need}"))
                .failed(),
        )
    }

    fn apply_setting(&mut self, key: &str, value: &str) -> ToolCall {
        let v = value.trim().to_lowercase();
        let num = v
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse::<u32>()
            .ok();
        let on = matches!(v.as_str(), "on" | "true" | "yes" | "1" | "enabled");
        let off = matches!(v.as_str(), "off" | "false" | "no" | "0" | "disabled");
        match key {
            "focus" | "pomodoro" => match num {
                Some(n) => {
                    self.state.settings.focus = n.clamp(1, 180);
                    if !self.timer.running && self.timer.mode == Mode::Focus {
                        self.timer.set_mode(&self.state.settings, Mode::Focus);
                    }
                    ToolCall::new("pomocard.settings")
                        .kv("focus", format!("{} min", self.state.settings.focus))
                        .kv("result", "saved")
                }
                None => ToolCall::new("pomocard.settings").kv("error", "expected a number").failed(),
            },
            "short" => match num {
                Some(n) => {
                    self.state.settings.short = n.clamp(1, 60);
                    ToolCall::new("pomocard.settings")
                        .kv("short", format!("{} min", self.state.settings.short))
                        .kv("result", "saved")
                }
                None => ToolCall::new("pomocard.settings").kv("error", "expected a number").failed(),
            },
            "long" => match num {
                Some(n) => {
                    self.state.settings.long = n.clamp(1, 60);
                    ToolCall::new("pomocard.settings")
                        .kv("long", format!("{} min", self.state.settings.long))
                        .kv("result", "saved")
                }
                None => ToolCall::new("pomocard.settings").kv("error", "expected a number").failed(),
            },
            "auto" | "autostart" => {
                self.state.settings.auto = if off { false } else { on || !self.state.settings.auto };
                ToolCall::new("pomocard.settings")
                    .kv("auto", bool_label(self.state.settings.auto))
                    .kv("result", "saved")
            }
            "sound" | "chime" => {
                if ["classic", "pad", "click", "none"].contains(&v.as_str()) {
                    self.state.settings.sound_theme = v.clone();
                    self.state.settings.sound = v != "none";
                    ToolCall::new("pomocard.settings")
                        .kv("chime", v)
                        .kv("result", "saved")
                } else {
                    self.state.settings.sound = if off { false } else { on || !self.state.settings.sound };
                    ToolCall::new("pomocard.settings")
                        .kv("sound", bool_label(self.state.settings.sound))
                        .kv("result", "saved")
                }
            }
            "ambient" => {
                self.state.ambient = if v == "off" || v == "none" {
                    None
                } else {
                    Some(v.clone())
                };
                ToolCall::new("pomocard.ambient")
                    .kv("engine", self.state.ambient.clone().unwrap_or_else(|| "off".into()))
                    .kv("note", "audio synthesis runs in the browser build")
            }
            "theme" => {
                self.state.theme = if v == "light" { "light".into() } else { "dark".into() };
                ToolCall::new("pomocard.theme").kv("theme", self.state.theme.clone()).kv("result", "applied")
            }
            "key" | "openrouter" | "apikey" => {
                let v = value.trim();
                self.state.settings.openrouter_key = if v.is_empty() || v == "off" || v == "none" || v == "clear" {
                    None
                } else {
                    Some(v.to_string())
                };
                ToolCall::new("pomocard.settings")
                    .kv(
                        "openrouter_key",
                        if self.state.settings.openrouter_key.is_some() { "set (hidden)" } else { "cleared" },
                    )
                    .kv("result", "saved")
            }
            "model" => {
                let v = value.trim();
                self.state.settings.model = if v.is_empty() || v == "off" || v == "none" {
                    crate::state::default_model()
                } else {
                    v.to_string()
                };
                ToolCall::new("pomocard.settings")
                    .kv("model", self.state.settings.model.clone())
                    .kv("result", "saved")
            }
            "plan" | "tier" => {
                let tier = match v.as_str() {
                    "team" => "team",
                    "pro" => "pro",
                    _ => "free",
                };
                self.state.tier = tier.into();
                ToolCall::new("pomocard.plan.set").kv("plan", tier_label(tier)).kv("result", "saved")
            }
            _ => ToolCall::new("pomocard.settings")
                .kv("key", key.to_string())
                .kv("known", "focus · short · long · auto · chime · ambient · theme · plan")
                .kv("error", "unknown setting")
                .failed(),
        }
    }
}

pub fn bool_label(v: bool) -> &'static str {
    if v {
        "on"
    } else {
        "off"
    }
}

fn short_session_id() -> String {
    let id = crate::state::uid();
    let body: String = id.chars().skip(1).collect();
    let (a, b) = body.split_at(4.min(body.len()));
    format!("{a}-{}", &b[..2.min(b.len())])
}

fn pretty_cwd() -> String {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let home = crate::state::home_dir();
    let s = cwd.display().to_string().replace('\\', "/");
    let h = home.display().to_string().replace('\\', "/");
    if !h.is_empty() && s.starts_with(&h) {
        format!("~{}", &s[h.len()..])
    } else {
        s
    }
}
