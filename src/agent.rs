//! The agent harness: turns free-form lines like
//! `start a 25m focus block and drop "Ship the landing page" into Today`
//! into a list of tool calls, exactly like the terminal mock on the landing page.

use crate::state::normalize_col;

/* ---------------- transcript ---------------- */

#[derive(Debug, Clone)]
pub struct ToolCall {
    pub name: String,
    pub rows: Vec<(String, String)>,
    pub ok: bool,
}

impl ToolCall {
    pub fn new(name: &str) -> ToolCall {
        ToolCall {
            name: name.to_string(),
            rows: Vec::new(),
            ok: true,
        }
    }

    pub fn kv(mut self, key: &str, value: impl Into<String>) -> ToolCall {
        self.rows.push((key.to_string(), value.into()));
        self
    }

    pub fn failed(mut self) -> ToolCall {
        self.ok = false;
        self
    }

    pub fn status(&self) -> &'static str {
        if self.ok {
            "✓ done"
        } else {
            "× failed"
        }
    }
}

#[derive(Debug, Clone)]
pub enum Entry {
    You(String),
    Agent(String),
    Tool(ToolCall),
    Note(String),
}

/* ---------------- commands ---------------- */

#[derive(Debug, Clone, PartialEq)]
pub enum Cmd {
    Start { minutes: Option<u32> },
    Pause,
    Reset,
    Skip,
    SetMode { mode: String, minutes: Option<u32> },
    Add { title: String, col: String, est: u8 },
    Move { query: String, col: String },
    Rename { query: String, title: String },
    Delete { query: String },
    Pin { query: String },
    Unpin,
    Est { query: String, est: u8 },
    ClearDone,
    ListBoard,
    Stats,
    Template { name: String },
    Upgrade { tier: String },
    Theme { theme: String },
    Set { key: String, value: String },
    View { name: String },
    Sync,
    Export,
    Help,
    Quit,
    Ask { text: String },
}

/// Split a line into clauses on `and` / `then` / `;` / `.` outside quotes.
pub fn split_clauses(input: &str) -> Vec<String> {
    let chars: Vec<char> = input.chars().collect();
    let lower: Vec<char> = input.to_lowercase().chars().collect();
    let seps = [" and then ", " then ", " and ", "; ", ";", ", then "];
    let mut out: Vec<String> = Vec::new();
    let mut buf = String::new();
    let mut quote: Option<char> = None;
    let mut i = 0usize;

    while i < chars.len() {
        let c = chars[i];
        match quote {
            Some(q) => {
                buf.push(c);
                if c == q || (q == '“' && c == '”') || (q == '‘' && c == '’') {
                    quote = None;
                }
                i += 1;
                continue;
            }
            None => {
                if c == '"' || c == '\'' || c == '“' || c == '‘' {
                    quote = Some(c);
                    buf.push(c);
                    i += 1;
                    continue;
                }
            }
        }

        let mut matched = 0usize;
        for sep in seps {
            let s: Vec<char> = sep.chars().collect();
            if i + s.len() <= lower.len() && lower[i..i + s.len()] == s[..] {
                matched = s.len();
                break;
            }
        }
        if matched > 0 {
            if !buf.trim().is_empty() {
                out.push(buf.trim().to_string());
            }
            buf.clear();
            i += matched;
            continue;
        }

        buf.push(c);
        i += 1;
    }
    if !buf.trim().is_empty() {
        out.push(buf.trim().to_string());
    }
    if out.is_empty() {
        out.push(input.trim().to_string());
    }
    out
}

pub fn parse(input: &str) -> Vec<Cmd> {
    split_clauses(input)
        .iter()
        .filter(|c| !c.trim().is_empty())
        .map(|c| parse_clause(c))
        .collect()
}

fn quoted_all(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut quote: Option<char> = None;
    for c in s.chars() {
        match quote {
            Some(q) => {
                let closing = matches!(
                    (q, c),
                    ('"', '"') | ('\'', '\'') | ('“', '”') | ('‘', '’')
                );
                if closing {
                    out.push(buf.trim().to_string());
                    buf.clear();
                    quote = None;
                } else {
                    buf.push(c);
                }
            }
            None => {
                if c == '"' || c == '\'' || c == '“' || c == '‘' {
                    quote = Some(c);
                }
            }
        }
    }
    out.retain(|s| !s.is_empty());
    out
}

fn detect_col(lower: &str) -> Option<&'static str> {
    for (needle, col) in [
        ("in progress", "doing"),
        ("in-progress", "doing"),
        ("doing", "doing"),
        ("wip", "doing"),
        ("backlog", "backlog"),
        ("today", "today"),
        ("todo", "today"),
        ("done", "done"),
        ("finished", "done"),
        ("complete", "done"),
    ] {
        if lower.contains(needle) {
            return normalize_col(col);
        }
    }
    None
}

/// `25m`, `25 min`, `1h`, `90 minutes`.
fn detect_minutes(lower: &str) -> Option<u32> {
    let bytes: Vec<char> = lower.chars().collect();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            let num: u32 = bytes[start..i].iter().collect::<String>().parse().ok()?;
            let mut j = i;
            while j < bytes.len() && bytes[j] == ' ' {
                j += 1;
            }
            let rest: String = bytes[j..].iter().collect();
            if rest.starts_with("hour") || rest.starts_with('h') {
                return Some(num * 60);
            }
            if rest.starts_with("min") || rest.starts_with('m') {
                return Some(num);
            }
            continue;
        }
        i += 1;
    }
    None
}

fn detect_est(lower: &str) -> Option<u8> {
    for key in ["est ", "estimate ", "x", "pomodoro", "pomodoros", "poms"] {
        if let Some(pos) = lower.find(key) {
            let tail: String = lower[pos..].chars().skip(key.len()).collect();
            let head: String = lower[..pos].trim_end().to_string();
            for candidate in [tail, head.chars().rev().collect::<String>()] {
                let digits: String = candidate
                    .trim()
                    .chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                if let Ok(n) = digits.parse::<u8>() {
                    if (1..=4).contains(&n) {
                        return Some(n);
                    }
                }
            }
        }
    }
    None
}

fn strip_leading(s: &str, words: &[&str]) -> String {
    let mut out = s.trim().to_string();
    loop {
        let lower = out.to_lowercase();
        let mut cut = 0usize;
        for w in words {
            let with_space = format!("{w} ");
            if lower.starts_with(&with_space) {
                cut = w.len() + 1;
                break;
            }
            if lower == *w {
                cut = w.len();
                break;
            }
        }
        if cut == 0 {
            return out.trim().to_string();
        }
        out = out[cut..].trim().to_string();
    }
}

/// Removes a trailing `to/into/in <column>` phrase from a title.
fn strip_col_phrase(s: &str) -> String {
    let lower = s.to_lowercase();
    let mut best: Option<usize> = None;
    for prep in [" into ", " to ", " in ", " on ", " under "] {
        let mut from = 0usize;
        while let Some(pos) = lower[from..].find(prep) {
            let abs = from + pos;
            let tail = &lower[abs + prep.len()..];
            let tail_clean = tail.trim().trim_end_matches(['.', '!', '?']);
            let tail_words = tail_clean
                .replace("the ", "")
                .replace(" column", "")
                .replace(" list", "")
                .trim()
                .to_string();
            if normalize_col(&tail_words).is_some() {
                // keep the earliest valid cut: "… to in progress" trims at " to "
                best = Some(match best {
                    Some(prev) => prev.min(abs),
                    None => abs,
                });
            }
            from = abs + prep.len();
        }
    }
    match best {
        Some(pos) => s[..pos].trim().to_string(),
        None => s.trim().to_string(),
    }
}

fn clean_title(raw: &str) -> String {
    let t = strip_col_phrase(raw);
    let t = t
        .trim()
        .trim_end_matches(['.', ',', '!', '?'])
        .trim_matches('"')
        .trim_matches('\'')
        .trim();
    let t = strip_leading(t, &["a", "an", "the", "new", "card", "task", "card:", "task:"]);
    t.trim().to_string()
}

fn has_word(lower: &str, word: &str) -> bool {
    let mut from = 0usize;
    while let Some(pos) = lower[from..].find(word) {
        let abs = from + pos;
        let before_ok = abs == 0
            || !lower[..abs]
                .chars()
                .next_back()
                .map(|c| c.is_alphanumeric())
                .unwrap_or(false);
        let after = abs + word.len();
        let after_ok = after >= lower.len()
            || !lower[after..]
                .chars()
                .next()
                .map(|c| c.is_alphanumeric())
                .unwrap_or(false);
        if before_ok && after_ok {
            return true;
        }
        from = abs + word.len();
    }
    false
}

fn any_word(lower: &str, words: &[&str]) -> bool {
    words.iter().any(|w| has_word(lower, w))
}

pub fn parse_clause(raw: &str) -> Cmd {
    let text = raw.trim();
    let lower = text.to_lowercase();
    let quoted = quoted_all(text);
    let col = detect_col(&lower);
    let minutes = detect_minutes(&lower);
    let est = detect_est(&lower).unwrap_or(1);

    if text.is_empty() {
        return Cmd::Ask { text: String::new() };
    }

    // explicit slash/bare commands first
    let head = lower.split_whitespace().next().unwrap_or("");
    match head.trim_start_matches('/') {
        "help" | "?" | "h" => return Cmd::Help,
        "quit" | "exit" | "q" | ":q" => return Cmd::Quit,
        "sync" => return Cmd::Sync,
        "export" => return Cmd::Export,
        _ => {}
    }

    if any_word(&lower, &["help", "commands"]) && lower.split_whitespace().count() <= 2 {
        return Cmd::Help;
    }

    // settings: "set focus 50", "set auto on", "set chime pad"
    if head == "set" || lower.starts_with("change ") {
        let parts: Vec<&str> = lower.split_whitespace().collect();
        if parts.len() >= 3 {
            return Cmd::Set {
                key: parts[1].to_string(),
                value: parts[2..].join(" "),
            };
        }
    }

    if any_word(&lower, &["theme", "dark", "light"]) && !any_word(&lower, &["card", "add"]) {
        let theme = if lower.contains("light") {
            "light"
        } else if lower.contains("dark") {
            "dark"
        } else {
            "toggle"
        };
        return Cmd::Theme {
            theme: theme.to_string(),
        };
    }

    if any_word(&lower, &["upgrade", "unlock", "subscribe"]) {
        let tier = if lower.contains("team") { "team" } else { "pro" };
        return Cmd::Upgrade {
            tier: tier.to_string(),
        };
    }
    if any_word(&lower, &["downgrade", "cancel"]) && !any_word(&lower, &["card"]) {
        return Cmd::Upgrade {
            tier: "free".to_string(),
        };
    }

    if any_word(&lower, &["template", "routine", "seed"]) {
        let name = quoted.first().cloned().unwrap_or_else(|| {
            strip_leading(
                &lower,
                &["load", "use", "apply", "start", "the", "template", "routine", "seed"],
            )
            .replace("template", "")
            .replace("routine", "")
            .trim()
            .to_string()
        });
        return Cmd::Template { name };
    }

    if lower.starts_with("clear done") || (has_word(&lower, "clear") && has_word(&lower, "done")) {
        return Cmd::ClearDone;
    }

    // timer verbs
    if any_word(&lower, &["pause", "hold", "wait"]) || lower.starts_with("stop") {
        return Cmd::Pause;
    }
    if any_word(&lower, &["reset", "restart"]) {
        return Cmd::Reset;
    }
    if any_word(&lower, &["skip", "next"]) && !any_word(&lower, &["card"]) {
        return Cmd::Skip;
    }
    if any_word(&lower, &["break"]) {
        let mode = if lower.contains("long") { "long" } else { "short" };
        return Cmd::SetMode {
            mode: mode.to_string(),
            minutes,
        };
    }

    let is_add_verb = any_word(&lower, &["add", "create", "capture", "queue", "drop", "put", "log"]);
    let is_move_verb = any_word(&lower, &["move", "push", "send", "shift", "promote"]);
    // "done" verbs only count at the head of a clause, otherwise a card called
    // "Ship the landing page" would look like a completion request.
    let is_done_verb = ["finish", "complete", "mark ", "done ", "ship ", "archive"]
        .iter()
        .any(|v| lower.starts_with(v));
    let is_del_verb = any_word(&lower, &["delete", "remove", "trash", "kill"]);
    let is_pin_verb = any_word(&lower, &["pin", "work"]) || lower.starts_with("focus on");

    if any_word(&lower, &["start", "begin", "resume", "go"])
        && !is_add_verb
        && !is_move_verb
    {
        if lower.contains("short break") || lower.contains("long break") {
            let mode = if lower.contains("long") { "long" } else { "short" };
            return Cmd::SetMode {
                mode: mode.to_string(),
                minutes,
            };
        }
        return Cmd::Start { minutes };
    }

    if is_del_verb {
        let q = quoted
            .first()
            .cloned()
            .unwrap_or_else(|| clean_title(&strip_leading(text, &["delete", "remove", "trash", "kill"])));
        return Cmd::Delete { query: q };
    }

    if is_pin_verb {
        if any_word(&lower, &["unpin", "nothing", "clear"]) {
            return Cmd::Unpin;
        }
        let q = quoted.first().cloned().unwrap_or_else(|| {
            clean_title(&strip_leading(
                text,
                &["pin", "focus", "on", "work", "let's", "lets", "start"],
            ))
        });
        return Cmd::Pin { query: q };
    }

    if any_word(&lower, &["rename", "retitle", "call"]) && quoted.len() >= 2 {
        return Cmd::Rename {
            query: quoted[0].clone(),
            title: quoted[1].clone(),
        };
    }

    if any_word(&lower, &["est", "estimate", "pomodoros"]) && (is_move_verb || !is_add_verb) {
        if let Some(n) = detect_est(&lower) {
            let q = quoted.first().cloned().unwrap_or_else(|| {
                clean_title(&strip_leading(text, &["set", "est", "estimate"]))
            });
            if !q.is_empty() {
                return Cmd::Est { query: q, est: n };
            }
        }
    }

    if is_done_verb && !is_add_verb && !is_move_verb {
        let q = quoted.first().cloned().unwrap_or_else(|| {
            clean_title(&strip_leading(
                text,
                &["mark", "finish", "complete", "completed", "ship", "shipped", "done", "archive", "as"],
            ))
        });
        return Cmd::Move {
            query: q,
            col: "done".to_string(),
        };
    }

    if is_move_verb {
        let q = quoted.first().cloned().unwrap_or_else(|| {
            clean_title(&strip_leading(
                text,
                &["move", "push", "send", "shift", "promote"],
            ))
        });
        return Cmd::Move {
            query: q,
            col: col.unwrap_or("today").to_string(),
        };
    }

    if is_add_verb {
        let title = quoted.first().cloned().unwrap_or_else(|| {
            clean_title(&strip_leading(
                text,
                &["add", "create", "capture", "queue", "drop", "put", "log"],
            ))
        });
        return Cmd::Add {
            title,
            col: col.unwrap_or("today").to_string(),
            est,
        };
    }

    if any_word(&lower, &["board", "cards", "list", "show"]) && !any_word(&lower, &["stats"]) {
        if let Some(view) = view_name(&lower) {
            return Cmd::View { name: view };
        }
        return Cmd::ListBoard;
    }

    if any_word(&lower, &["stats", "status", "streak", "progress", "summary"]) {
        return Cmd::Stats;
    }

    if let Some(view) = view_name(&lower) {
        if any_word(&lower, &["open", "go", "show", "view", "switch"]) || lower.split_whitespace().count() <= 2 {
            return Cmd::View { name: view };
        }
    }

    Cmd::Ask {
        text: text.to_string(),
    }
}

fn view_name(lower: &str) -> Option<String> {
    for name in [
        "agent",
        "board",
        "analytics",
        "coach",
        "templates",
        "team",
        "billing",
        "settings",
    ] {
        if has_word(lower, name) {
            return Some(name.to_string());
        }
    }
    if has_word(lower, "kanban") {
        return Some("board".into());
    }
    if has_word(lower, "insights") {
        return Some("analytics".into());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_the_landing_page_example() {
        let cmds = parse("start a 25m focus block and drop \"Ship the landing page\" into Today.");
        assert_eq!(cmds.len(), 2);
        assert_eq!(cmds[0], Cmd::Start { minutes: Some(25) });
        assert_eq!(
            cmds[1],
            Cmd::Add {
                title: "Ship the landing page".into(),
                col: "today".into(),
                est: 1
            }
        );
    }

    #[test]
    fn parses_unquoted_add() {
        assert_eq!(
            parse_clause("add write the docs to backlog"),
            Cmd::Add {
                title: "write the docs".into(),
                col: "backlog".into(),
                est: 1
            }
        );
    }

    #[test]
    fn parses_move_and_done() {
        assert_eq!(
            parse_clause("move \"Refactor timer module\" to in progress"),
            Cmd::Move {
                query: "Refactor timer module".into(),
                col: "doing".into()
            }
        );
        assert_eq!(
            parse_clause("finish \"Draft chapter 3\""),
            Cmd::Move {
                query: "Draft chapter 3".into(),
                col: "done".into()
            }
        );
    }

    #[test]
    fn move_beats_a_card_titled_ship() {
        assert_eq!(
            parse_clause("move Ship the landing page to in progress"),
            Cmd::Move {
                query: "Ship the landing page".into(),
                col: "doing".into()
            }
        );
    }

    #[test]
    fn parses_timer_verbs() {
        assert_eq!(parse_clause("pause"), Cmd::Pause);
        assert_eq!(parse_clause("reset the timer"), Cmd::Reset);
        assert_eq!(parse_clause("skip"), Cmd::Skip);
        assert_eq!(
            parse_clause("take a long break"),
            Cmd::SetMode {
                mode: "long".into(),
                minutes: None
            }
        );
    }

    #[test]
    fn unknown_text_goes_to_the_coach() {
        match parse_clause("why do I keep losing focus after lunch") {
            Cmd::Ask { .. } => {}
            other => panic!("expected Ask, got {other:?}"),
        }
    }
}
