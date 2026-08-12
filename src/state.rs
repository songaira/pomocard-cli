//! Persistent state — wire-compatible with the web app's `localStorage["pomocard.v2"]`.
//!
//! The browser build keeps everything in one JSON blob. The CLI reads/writes the
//! very same shape from a file so you can copy it in and out of the browser
//! (`localStorage.setItem('pomocard.v2', <contents>)`) without losing anything.
//! Unknown keys written by the web app are preserved via `extra`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use chrono::{Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const COLS: [&str; 4] = ["backlog", "today", "doing", "done"];

pub fn col_label(col: &str) -> &'static str {
    match col {
        "today" => "Today",
        "doing" => "In progress",
        "done" => "Done",
        _ => "Backlog",
    }
}

/// Loose column matching: "in progress", "wip", "doing" all mean the same thing.
pub fn normalize_col(raw: &str) -> Option<&'static str> {
    let s = raw.trim().to_lowercase();
    match s.as_str() {
        "backlog" | "inbox" | "later" | "someday" => Some("backlog"),
        "today" | "todo" | "to do" | "now" => Some("today"),
        "doing" | "in progress" | "in-progress" | "progress" | "wip" | "active" => Some("doing"),
        "done" | "complete" | "completed" | "finished" | "shipped" => Some("done"),
        _ => None,
    }
}

/* ---------------- settings ---------------- */

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(default = "d_focus")]
    pub focus: u32,
    #[serde(default = "d_short")]
    pub short: u32,
    #[serde(default = "d_long")]
    pub long: u32,
    #[serde(default)]
    pub auto: bool,
    #[serde(default = "d_true")]
    pub sound: bool,
    #[serde(default = "d_chime")]
    pub sound_theme: String,
    /// Bring-your-own-key OpenRouter token. When set, natural-language input is
    /// translated by a model (see `llm`). Stored locally; never sent anywhere
    /// except OpenRouter's API on each request.
    #[serde(default)]
    pub openrouter_key: Option<String>,
    /// OpenRouter model id. Defaults to a `:free` model so the free tier costs
    /// nobody anything.
    #[serde(default = "d_model")]
    pub model: String,
}

fn d_focus() -> u32 {
    25
}
fn d_short() -> u32 {
    5
}
fn d_long() -> u32 {
    15
}
fn d_true() -> bool {
    true
}
fn d_chime() -> String {
    "classic".into()
}
fn d_model() -> String {
    "qwen/qwen3-8b:free".into()
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            focus: 25,
            short: 5,
            long: 15,
            auto: false,
            sound: true,
            sound_theme: "classic".into(),
            openrouter_key: None,
            model: d_model(),
        }
    }
}

/// The default (free) OpenRouter model id.
pub fn default_model() -> String {
    d_model()
}

/* ---------------- stats ---------------- */

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Stats {
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub minutes: u32,
    #[serde(default)]
    pub sessions: u32,
    #[serde(default)]
    pub streak: u32,
    #[serde(default)]
    pub last: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Totals {
    #[serde(default)]
    pub minutes: u32,
    #[serde(default)]
    pub sessions: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryDay {
    pub d: String,
    pub min: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Achievement {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub plan: String,
}

/* ---------------- board ---------------- */

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Card {
    pub id: String,
    pub title: String,
    #[serde(default = "one")]
    pub est: u8,
    #[serde(default)]
    pub active: bool,
}

fn one() -> u8 {
    1
}

impl Card {
    pub fn new(title: impl Into<String>, est: u8) -> Card {
        Card {
            id: uid(),
            title: title.into(),
            est: est.clamp(1, 4),
            active: false,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Board {
    #[serde(default)]
    pub backlog: Vec<Card>,
    #[serde(default)]
    pub today: Vec<Card>,
    #[serde(default)]
    pub doing: Vec<Card>,
    #[serde(default)]
    pub done: Vec<Card>,
}

impl Board {
    pub fn column(&self, col: &str) -> &Vec<Card> {
        match col {
            "today" => &self.today,
            "doing" => &self.doing,
            "done" => &self.done,
            _ => &self.backlog,
        }
    }

    pub fn column_mut(&mut self, col: &str) -> &mut Vec<Card> {
        match col {
            "today" => &mut self.today,
            "doing" => &mut self.doing,
            "done" => &mut self.done,
            _ => &mut self.backlog,
        }
    }

    pub fn total(&self) -> usize {
        COLS.iter().map(|c| self.column(c).len()).sum()
    }

    pub fn est_total(&self) -> u32 {
        COLS.iter()
            .flat_map(|c| self.column(c).iter())
            .map(|c| c.est as u32)
            .sum()
    }

    pub fn find(&self, id: &str) -> Option<(&'static str, usize)> {
        for col in COLS {
            if let Some(i) = self.column(col).iter().position(|c| c.id == id) {
                return Some((col, i));
            }
        }
        None
    }

    /// Fuzzy-ish lookup used by the agent: exact id, then exact title,
    /// then "starts with", then "contains" — all case-insensitive.
    pub fn search(&self, query: &str) -> Option<(&'static str, usize)> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return None;
        }
        if let Some(hit) = self.find(&q) {
            return Some(hit);
        }
        let mut starts: Option<(&'static str, usize)> = None;
        let mut contains: Option<(&'static str, usize)> = None;
        for col in COLS {
            for (i, card) in self.column(col).iter().enumerate() {
                let t = card.title.to_lowercase();
                if t == q {
                    return Some((col, i));
                }
                if starts.is_none() && t.starts_with(&q) {
                    starts = Some((col, i));
                }
                if contains.is_none() && t.contains(&q) {
                    contains = Some((col, i));
                }
            }
        }
        starts.or(contains)
    }
}

/* ---------------- state ---------------- */

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    #[serde(default = "d_theme")]
    pub theme: String,
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub session: u32,
    #[serde(default)]
    pub active_id: Option<String>,
    #[serde(default)]
    pub stats: Stats,
    #[serde(default)]
    pub board: Board,
    #[serde(default)]
    pub account: Option<Account>,
    #[serde(default = "d_tier")]
    pub tier: String,
    #[serde(default)]
    pub totals: Totals,
    #[serde(default)]
    pub achievements: Vec<Achievement>,
    #[serde(default)]
    pub xp: u32,
    #[serde(default)]
    pub history: Vec<HistoryDay>,
    #[serde(default)]
    pub ambient: Option<String>,
    /// Anything the browser build writes that the CLI does not model.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

fn d_theme() -> String {
    "dark".into()
}
fn d_tier() -> String {
    "free".into()
}

impl Default for State {
    fn default() -> Self {
        State {
            theme: d_theme(),
            settings: Settings::default(),
            session: 0,
            active_id: None,
            stats: Stats::default(),
            board: Board::default(),
            account: None,
            tier: d_tier(),
            totals: Totals::default(),
            achievements: Vec::new(),
            xp: 0,
            history: Vec::new(),
            ambient: None,
            extra: Map::new(),
        }
    }
}

pub fn tier_rank(tier: &str) -> u8 {
    match tier {
        "pro" => 1,
        "team" => 2,
        _ => 0,
    }
}

pub fn tier_label(tier: &str) -> &'static str {
    match tier {
        "pro" => "Pro",
        "team" => "Team",
        _ => "Free",
    }
}

pub fn tier_price(tier: &str) -> &'static str {
    match tier {
        "pro" => "$6/mo",
        "team" => "$12/user/mo",
        _ => "$0",
    }
}

impl State {
    /// Same starter board the web app seeds.
    pub fn seed() -> State {
        let mut s = State::default();
        s.board.backlog = vec![
            Card::new("Outline the Q3 report", 2),
            Card::new("Read design system docs", 1),
        ];
        s.board.today = vec![
            Card::new("Write landing page copy", 3),
            Card::new("Refactor timer module", 2),
        ];
        s.board.doing = vec![Card::new("Implement drag & drop", 4)];
        s.board.done = vec![
            Card::new("Set up project repo", 1),
            Card::new("Pick monochrome palette", 1),
        ];
        s
    }

    pub fn tier_ok(&self, need: &str) -> bool {
        tier_rank(&self.tier) >= tier_rank(need)
    }

    pub fn active_card(&self) -> Option<&Card> {
        let id = self.active_id.as_ref()?;
        let (col, i) = self.board.find(id)?;
        self.board.column(col).get(i)
    }

    /// Mirrors `recordFocusSession()` in app.js (streak roll-over included).
    pub fn record_focus_session(&mut self, minutes: u32) {
        let today = today_key();
        if self.stats.date != today {
            if !self.stats.last.is_empty() && self.stats.last == yesterday_key() {
                self.stats.streak += 1;
            } else {
                self.stats.streak = 1;
            }
            self.stats.date = today.clone();
            self.stats.minutes = 0;
            self.stats.sessions = 0;
        }
        self.stats.minutes += minutes;
        self.stats.sessions += 1;
        self.stats.last = today.clone();
        self.totals.minutes += minutes;
        self.totals.sessions += 1;
        self.xp += 10;

        match self.history.iter_mut().find(|h| h.d == today) {
            Some(day) => day.min += minutes,
            None => self.history.push(HistoryDay {
                d: today,
                min: minutes,
            }),
        }
        if self.history.len() > 400 {
            let cut = self.history.len() - 400;
            self.history.drain(0..cut);
        }
    }

    pub fn minutes_on(&self, day: &str) -> u32 {
        self.history
            .iter()
            .find(|h| h.d == day)
            .map(|h| h.min)
            .unwrap_or(0)
    }

    /* ---- board mutations ---- */

    pub fn add_card(&mut self, col: &str, title: &str, est: u8) -> String {
        let card = Card::new(title.trim(), est);
        let id = card.id.clone();
        self.board.column_mut(col).push(card);
        id
    }

    pub fn move_card(&mut self, id: &str, to_col: &str, to_idx: Option<usize>) -> bool {
        let Some((from, i)) = self.board.find(id) else {
            return false;
        };
        let mut card = self.board.column_mut(from).remove(i);
        if to_col == "done" {
            card.active = false;
            if self.active_id.as_deref() == Some(id) {
                self.active_id = None;
            }
        }
        let target = self.board.column_mut(to_col);
        let idx = to_idx.unwrap_or(target.len()).min(target.len());
        target.insert(idx, card);
        true
    }

    pub fn delete_card(&mut self, id: &str) -> Option<Card> {
        let (col, i) = self.board.find(id)?;
        if self.active_id.as_deref() == Some(id) {
            self.active_id = None;
        }
        Some(self.board.column_mut(col).remove(i))
    }

    pub fn set_active(&mut self, id: Option<&str>) {
        for col in COLS {
            for card in self.board.column_mut(col).iter_mut() {
                card.active = Some(card.id.as_str()) == id;
            }
        }
        self.active_id = id.map(|s| s.to_string());
    }

    pub fn clear_done(&mut self) -> usize {
        let n = self.board.done.len();
        self.board.done.clear();
        n
    }

    /* ---- persistence ---- */

    pub fn load(path: &Path) -> Result<State> {
        if !path.exists() {
            return Ok(State::seed());
        }
        let raw = fs::read_to_string(path)
            .with_context(|| format!("reading state from {}", path.display()))?;
        if raw.trim().is_empty() {
            return Ok(State::seed());
        }
        let state: State = serde_json::from_str(&raw)
            .with_context(|| format!("parsing state file {}", path.display()))?;
        Ok(state)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            if !dir.as_os_str().is_empty() {
                fs::create_dir_all(dir)
                    .with_context(|| format!("creating {}", dir.display()))?;
            }
        }
        let json = serde_json::to_string_pretty(self)?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, json.as_bytes())
            .with_context(|| format!("writing {}", tmp.display()))?;
        // rename() replaces on unix, fails on windows if dest exists -> remove first
        if path.exists() {
            let _ = fs::remove_file(path);
        }
        fs::rename(&tmp, path).with_context(|| format!("saving {}", path.display()))?;
        Ok(())
    }
}

/* ---------------- helpers ---------------- */

static COUNTER: AtomicU64 = AtomicU64::new(1);

/// `c` + 7 base36 chars, same silhouette as the web app's `uid()`.
pub fn uid() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let bump = COUNTER.fetch_add(0x9E37_79B9, Ordering::Relaxed);
    let mut x = nanos ^ bump.rotate_left(17) ^ 0x9E37_79B9_7F4A_7C15;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    let mut out = String::with_capacity(8);
    out.push('c');
    for _ in 0..7 {
        let d = (x % 36) as u32;
        x /= 36;
        out.push(char::from_digit(d, 36).unwrap_or('0'));
    }
    out
}

pub fn today_key() -> String {
    Utc::now().format("%Y-%m-%d").to_string()
}

pub fn yesterday_key() -> String {
    (Utc::now() - ChronoDuration::days(1))
        .format("%Y-%m-%d")
        .to_string()
}

pub fn home_dir() -> PathBuf {
    for key in ["POMOCARD_HOME", "USERPROFILE", "HOME"] {
        if let Ok(v) = std::env::var(key) {
            if !v.trim().is_empty() {
                return PathBuf::from(v);
            }
        }
    }
    PathBuf::from(".")
}

/// `~/.pomocard/pomocard.v2.json`, overridable with `POMOCARD_STATE` or `--state`.
pub fn default_state_path() -> PathBuf {
    if let Ok(v) = std::env::var("POMOCARD_STATE") {
        if !v.trim().is_empty() {
            return PathBuf::from(v);
        }
    }
    home_dir().join(".pomocard").join("pomocard.v2.json")
}

pub fn fmt_minutes(total: u32) -> String {
    let h = total / 60;
    let m = total % 60;
    if h > 0 {
        format!("{h}h {m:02}m")
    } else {
        format!("{m}m")
    }
}
