//! Application state machine shared by the TUI and the headless `exec` mode.

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Instant;

use anyhow::Result;

use crate::agent::{self, Cmd, Entry, ToolCall};
use crate::data::Data;
use crate::state::{
    col_label, fmt_minutes, normalize_col, Achievement, State, COLS,
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

const ACHS: [Ach; 5] = [
    Ach { id: "first", name: "First focus", desc: "Complete a session" },
    Ach { id: "streak3", name: "On a roll", desc: "3-day streak" },
    Ach { id: "ten", name: "In the zone", desc: "10 sessions" },
    Ach { id: "cycle", name: "Full cycle", desc: "4 sessions in a row" },
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
    /// `true` while an async LLM call is in flight (drives the spinner).
    pub thinking: bool,
    /// In-flight LLM request, resolved each tick by `poll_pending`.
    pub pending: Option<PendingLlm>,
}

/// A background LLM request: the receiving end of the channel the worker
/// thread sends its result on, plus enough context to finalize the turn.
pub struct PendingLlm {
    pub rx: mpsc::Receiver<Result<String, String>>,
    pub line: String,
    pub is_question: bool,
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
            thinking: false,
            pending: None,
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
        let persona = self.state.settings.persona_tail();
        if persona.is_empty() {
            format!("{}. {}", self.headline(), tail)
        } else {
            format!("{}. {}. {}", self.headline(), tail, persona)
        }
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
        self.view = view;
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
            PaletteAction { label: "Go to Analytics".into(), hint: "3".into(), cmd: "open analytics".into() },
            PaletteAction { label: "Go to AI Coach".into(), hint: "4".into(), cmd: "open coach".into() },
            PaletteAction { label: "Go to Templates".into(), hint: "5".into(), cmd: "open templates".into() },
            PaletteAction { label: "Go to Team console".into(), hint: "6".into(), cmd: "open team".into() },
            PaletteAction { label: "Go to Billing".into(), hint: "7".into(), cmd: "open billing".into() },
            PaletteAction { label: "Go to Settings".into(), hint: "8".into(), cmd: "open settings".into() },
            PaletteAction { label: "Toggle dark mode".into(), hint: "T".into(), cmd: "theme toggle".into() },
            PaletteAction { label: "Clear the Done column".into(), hint: "".into(), cmd: "clear done".into() },
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
    ///
    /// Headless (`exec`/`status`) runs fully synchronously and returns the
    /// entries for printing. In the TUI it kicks off a live, non-blocking turn
    /// so the `▸ thinking…` step shows immediately instead of after the (often
    /// multi-second) LLM round-trip.
    pub fn exec_line(&mut self, line: &str) -> Vec<Entry> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return Vec::new();
        }
        if self.headless {
            self.exec_line_blocking(trimmed)
        } else {
            self.exec_line_live(trimmed);
            Vec::new()
        }
    }

    /// Synchronous, headless execution (returns entries for printing).
    fn exec_line_blocking(&mut self, trimmed: &str) -> Vec<Entry> {
        let mut produced: Vec<Entry> = Vec::new();
        produced.push(Entry::You(trimmed.to_string()));

        let question = agent::is_question(trimmed);
        let has_key = self.state.settings.has_key();
        let local = agent::parse(trimmed);
        let needs_llm = has_key && local.iter().any(|c| matches!(c, Cmd::Ask { .. }));

        if needs_llm {
            produced.push(Entry::Note("▸ thinking…".into()));
        }

        let cmds = if needs_llm {
            if question {
                vec![Cmd::Ask {
                    text: trimmed.to_string(),
                }]
            } else {
                self.resolve_cmds(trimmed)
            }
        } else {
            local
        };
        if needs_llm && !question {
            produced.push(Entry::Note("▸ executing request…".into()));
        }
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

    /// Live TUI turn: show the `You` line, then run the LLM call on a worker
    /// thread so the UI keeps rendering while we wait (no thinking indicator).
    fn exec_line_live(&mut self, trimmed: &str) {
        self.transcript.push(Entry::You(trimmed.to_string()));
        self.follow = true;
        let question = agent::is_question(trimmed);

        if !self.state.settings.has_key() {
            // No key → offline local execution, no spinner needed.
            self.run_cmds(agent::parse(trimmed), false);
            self.award();
            self.save();
            return;
        }

        // The local parser already understands this as concrete commands
        // (no free-form `Ask`), so run it directly. This avoids the thinking
        // spinner — and a needless LLM round-trip — for explicit commands like
        // `exit`, `help`, `stats`, `add "…" to today`, etc.
        let local = agent::parse(trimmed);
        let needs_llm = local.iter().any(|c| matches!(c, Cmd::Ask { .. }));
        if !needs_llm {
            self.run_cmds(local, false);
            self.award();
            self.save();
            return;
        }

        // The LLM still does its work on a worker thread (so there's a natural
        // pause before the reply), but we no longer surface a `▸ thinking…`
        // step or spinner for it.
        self.follow = true;

        let (tx, rx) = mpsc::channel();
        let key = self.state.settings.resolve_key().unwrap();
        let provider = self.state.settings.provider.clone();
        let model = self.state.settings.model.clone();
        let line_owned = trimmed.to_string();
        let line_for_thread = line_owned.clone();
        let q = question;
        let augmented = self.augment(&line_for_thread);
        let agent_persona = self.state.settings.agent_persona.clone();
        let agent_custom = self.state.settings.agent_custom_prompt.clone();
        std::thread::spawn(move || {
            let res = if q {
                crate::llm::chat(
                    &key,
                    &provider,
                    &model,
                    &augmented,
                    &agent_persona,
                    agent_custom.as_deref(),
                )
            } else {
                crate::llm::complete(&key, &provider, &model, &augmented)
            };
            let _ = tx.send(res.map_err(|e| format!("{e:?}")));
        });
        self.pending = Some(PendingLlm {
            rx,
            line: line_owned,
            is_question: question,
        });
    }

    /// Called every tick from the TUI loop; resolves a finished LLM call.
    pub fn poll_pending(&mut self) {
        let pending = match self.pending.take() {
            Some(p) => p,
            None => return,
        };
        let outcome = match pending.rx.try_recv() {
            Ok(o) => o,
            Err(mpsc::TryRecvError::Empty) => {
                self.pending = Some(pending);
                return;
            }
            Err(_) => {
                self.thinking = false;
                return;
            }
        };
        self.thinking = false;
        if pending.is_question {
            self.finalize_ask(pending.line, outcome);
        } else {
            self.finalize_translate(pending.line, outcome);
        }
        self.follow = true;
        self.award();
        self.save();
    }

    /// Execute a batch of commands, emitting the `▸ executing request…` step
    /// when they came from an LLM translation.
    fn run_cmds(&mut self, cmds: Vec<Cmd>, used_llm: bool) {
        if used_llm {
            self.transcript.push(Entry::Note("▸ executing request…".into()));
        }
        let mut asked = false;
        for cmd in cmds {
            if matches!(cmd, Cmd::Ask { .. }) {
                asked = true;
            }
            let entries = self.exec_cmd(cmd);
            self.transcript.extend(entries);
        }
        if !asked {
            self.transcript.push(Entry::Agent(self.agent_summary()));
        }
        self.follow = true;
    }

    /// Finalize a question turn from an LLM chat result.
    fn finalize_ask(&mut self, text: String, outcome: Result<String, String>) {
        self.coach_feed.push((true, text.clone()));
        let reply = match outcome {
            Ok(r) => r.trim().to_string(),
            Err(err) => {
                self.toast(
                    "AI Coach unavailable",
                    format!("fell back to offline line ({err:?})"),
                );
                format!("[AI offline — {err}] {}", self.data.coach_line(0))
            }
        };
        let full = if self.state.settings.has_key() {
            reply
        } else {
            format!(
                "{reply}  (offline — `set provider` + `set key` for live AI)"
            )
        };
        self.coach_feed.push((false, full.clone()));
        self.transcript.push(Entry::Agent(full));
        self.follow = true;
    }

    /// Finalize a translation turn: turn the LLM text into commands and run them.
    fn finalize_translate(&mut self, line: String, outcome: Result<String, String>) {
        let (cmds, used_llm) = match outcome {
            Ok(text) => {
                let mut v = Vec::new();
                for l in text.lines() {
                    let l = l.trim();
                    if !l.is_empty() {
                        v.extend(agent::parse(l));
                    }
                }
                if v.is_empty() {
                    (agent::parse(&line), false)
                } else {
                    (v, true)
                }
            }
            Err(err) => {
                self.toast("AI translate failed", format!("{err:?}"));
                (agent::parse(&line), false)
            }
        };
        self.run_cmds(cmds, used_llm);
    }

    fn agent_context(&self) -> String {
        let board = &self.state.board;
        let mut s = String::new();
        s.push_str("[context - current board state]\nThe board currently contains these cards (title + column). When the user says \"that card\", \"it\", \"the previous task\", or \"the one I just made\", you MUST replace it with the EXACT title from the list. Never output the literal words that/it/previous/the one I just made as a title. Output only resolved command lines.\n");
        for col in COLS {
            let titles: Vec<String> = board.column(col).iter().map(|c| format!("\"{}\"", c.title)).collect();
            let label = col_label(col);
            if titles.is_empty() {
                s.push_str(&format!("- {}: (empty)\n", label));
            } else {
                s.push_str(&format!("- {} ({})\n", titles.join(", "), label));
            }
        }
        let recent: Vec<&Entry> = self.transcript.iter().rev().take(6).collect();
        if !recent.is_empty() {
            s.push_str("\n[recent activity]\n");
            for e in recent.iter().rev() {
                match e {
                    Entry::You(t) => s.push_str(&format!("user: {t}\n")),
                    Entry::Agent(t) => s.push_str(&format!("agent: {t}\n")),
                    Entry::Tool(t) => {
                        let parts: Vec<String> = t.rows.iter().map(|(k, v)| format!("{k}={v}")).collect();
                        s.push_str(&format!("tool {}: {}\n", t.name, parts.join(" ")));
                    }
                    Entry::Note(t) => s.push_str(&format!("note: {t}\n")),
                }
            }
        }
        s
    }

    fn augment(&self, line: &str) -> String {
        format!("{}\n\nRequest: {}", self.agent_context(), line)
    }
    /// Resolve a prompt into commands. With a provider key set, the configured
    /// model translates the request into local commands; otherwise we fall back
    /// to the built-in rule-based parser.
    fn resolve_cmds(&mut self, line: &str) -> Vec<Cmd> {
        if let Some(key) = self.state.settings.resolve_key() {
            if !key.trim().is_empty() {
                match crate::llm::complete(
                    &key,
                    &self.state.settings.provider,
                    &self.state.settings.model,
                    &self.augment(line),
                ) {
                    Ok(text) => {
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
                    Err(err) => self.toast("AI translate failed", format!("{err:?}")),
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
                        .kv("level", format!("{} ({} xp)", self.level(), self.state.xp)),
                ));
            }
            Cmd::Template { name } => {
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
                    self.go(v);
                    out.push(Entry::Tool(
                        ToolCall::new("pomocard.view").kv("view", v.title()).kv("result", "opened"),
                    ));
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
                        .kv("account", "local only")
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

                // Prefer a real reply from the configured provider's model. The
                // hardcoded coach lines are only the offline fallback when no
                // key/model is set (or the API call fails).
                let reply = if let Some(key) = self.state.settings.resolve_key() {
                    if !key.trim().is_empty() && !text.trim().is_empty() {
                        match crate::llm::chat(
                            &key,
                            &self.state.settings.provider,
                            &self.state.settings.model,
                            &self.augment(&text),
                            &self.state.settings.agent_persona,
                            self.state.settings.agent_custom_prompt.as_deref(),
                        ) {
                            Ok(r) => r.trim().to_string(),
                            Err(err) => {
                                // model id / network hiccup — surface it in the
                                // reply (and a toast) instead of failing silently.
                                self.toast(
                                    "AI Coach unavailable",
                                    format!("fell back to offline line ({err:?})"),
                                );
                                format!("[AI offline — {err}] {}", self.data.coach_line(n))
                            }
                        }
                    } else {
                        self.data.coach_line(n)
                    }
                } else {
                    self.data.coach_line(n)
                };

                let full = if self.state.settings.has_key() {
                    reply
                } else {
                    format!(
                        "{reply}  (offline — `set provider` + `set key` for live AI)"
                    )
                };
                self.coach_feed.push((false, full.clone()));
                out.push(Entry::Agent(full));
            }
        }
        out
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
                self.state.settings.set_key_for_provider(v.to_string());
                ToolCall::new("pomocard.settings")
                    .kv("provider", self.state.settings.provider.clone())
                    .kv(
                        "key",
                        if self.state.settings.has_key() { "set (hidden)" } else { "cleared" },
                    )
                    .kv("result", "saved")
            }
            "agent" => {
                let v = value.trim();
                let sp: Vec<&str> = v.split_whitespace().collect();
                if sp.is_empty() {
                    return ToolCall::new("pomocard.agent")
                        .kv(
                            "error",
                            "usage: set agent persona <warm|cold|stoic|chaotic|balanced> | set agent prompt <text> | set agent reset",
                        )
                        .failed();
                }
                match sp[0] {
                    "persona" => {
                        if sp.len() < 2 {
                            return ToolCall::new("pomocard.agent")
                                .kv("error", "usage: set agent persona <warm|cold|stoic|chaotic|balanced>")
                                .failed();
                        }
                        let p = sp[1].to_lowercase();
                        // Switching to a preset clears any custom prompt so the
                        // persona actually takes effect (they are mutually exclusive).
                        self.state.settings.agent_persona = p.clone();
                        self.state.settings.agent_custom_prompt = None;
                        ToolCall::new("pomocard.agent")
                            .kv("persona", p)
                            .kv("result", "applied")
                    }
                    "prompt" => {
                        let txt = sp[1..].join(" ");
                        if txt.trim().is_empty() {
                            return ToolCall::new("pomocard.agent")
                                .kv("error", "usage: set agent prompt <your system prompt>")
                                .failed();
                        }
                        self.state.settings.agent_custom_prompt = Some(txt);
                        ToolCall::new("pomocard.agent")
                            .kv("persona", "custom")
                            .kv("prompt", "set (hidden)")
                            .kv("result", "hotswapped")
                    }
                    "reset" => {
                        self.state.settings.agent_persona = "balanced".into();
                        self.state.settings.agent_custom_prompt = None;
                        ToolCall::new("pomocard.agent")
                            .kv("persona", "balanced")
                            .kv("result", "reset")
                    }
                    _ => ToolCall::new("pomocard.agent")
                        .kv("error", "unknown agent subcommand (persona|prompt|reset)")
                        .failed(),
                }
            }
            "model" => {
                let v = value.trim();
                self.state.settings.model = if v.is_empty() || v == "off" || v == "none" {
                    crate::llm::Provider::parse(&self.state.settings.provider)
                        .default_model()
                        .to_string()
                } else {
                    v.to_string()
                };
                ToolCall::new("pomocard.settings")
                    .kv("provider", self.state.settings.provider.clone())
                    .kv("model", self.state.settings.model.clone())
                    .kv("result", "saved")
            }
            "provider" => {
                let v = value.trim().to_lowercase();
                self.state.settings.provider = if v.is_empty() {
                    crate::state::default_provider().to_string()
                } else {
                    v
                };
                // reset the model to that provider's default so it works OOTB
                self.state.settings.model = crate::llm::Provider::parse(&self.state.settings.provider)
                    .default_model()
                    .to_string();
                ToolCall::new("pomocard.settings")
                    .kv("provider", self.state.settings.provider.clone())
                    .kv("model", self.state.settings.model.clone())
                    .kv("result", "saved")
            }
            _ => ToolCall::new("pomocard.settings")
                .kv("key", key.to_string())
                .kv("known", "focus · short · long · auto · chime · ambient · theme · provider · key · model")
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
