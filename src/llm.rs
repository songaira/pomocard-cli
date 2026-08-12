//! Optional Bring-Your-Own-Key LLM bridge via OpenRouter.
//!
//! Free/local by default. When an OpenRouter key is set, natural-language input
//! is translated by a `:free` model into the same commands the local parser
//! understands, then executed locally. Costs nobody anything.
//!
//! Uses the system `curl` for the HTTPS call so we pull in no TLS/async deps.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::process::Command;

const ENDPOINT: &str = "https://openrouter.ai/api/v1/chat/completions";

const SYSTEM: &str = "You are the Pomocard agent, a Pomodoro + Kanban CLI. \
Translate the user's request into one or more Pomocard natural-language commands, \
one per line, with NO extra text, numbering, or markdown. Each line must be a \
command the local parser understands, e.g.: \
add \"Write the spec\" to today; start a 25m focus block; move \"QA pass\" to in progress; \
finish \"Draft chapter\"; pin \"Refactor\"; delete \"Old task\"; estimate \"Big task\" 3; \
stats; clear done; open board; set focus 50; theme dark. \
Use the exact card titles given (quoted). Only output the command lines.";

#[derive(Deserialize)]
struct ORResponse {
    choices: Vec<Choice>,
}
#[derive(Deserialize)]
struct Choice {
    message: Message,
}
#[derive(Deserialize)]
struct Message {
    content: String,
}

/// Translate `user` text into Pomocard natural-language command lines.
pub fn complete(key: &str, model: &str, user: &str) -> Result<String> {
    let body = serde_json::json!({
        "model": model,
        "messages": [
            { "role": "system", "content": SYSTEM },
            { "role": "user", "content": user }
        ],
        "temperature": 0.2,
    })
    .to_string();

    let output = Command::new("curl")
        .args([
            "-s",
            "-S",
            "-X",
            "POST",
            ENDPOINT,
            "-H",
            &format!("Authorization: Bearer {}", key.trim()),
            "-H",
            "Content-Type: application/json",
            "-d",
            &body,
        ])
        .output()
        .context("failed to spawn `curl` — install curl (ships with Windows 10+, macOS, Linux)")?;

    if !output.status.success() {
        anyhow::bail!("OpenRouter request failed (curl exit {:?})", output.status.code());
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let resp: ORResponse =
        serde_json::from_str(&text).context("could not parse OpenRouter response")?;
    resp.choices
        .into_iter()
        .next()
        .map(|c| c.message.content)
        .filter(|c| !c.trim().is_empty())
        .context("OpenRouter returned no content")
}
