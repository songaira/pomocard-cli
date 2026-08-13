//! Optional Bring-Your-Own-Key LLM bridge.
//!
//! Free/local by default. When a provider key is set, natural-language input is
//! translated by a model into the same commands the local parser understands,
//! then executed locally. Costs nobody anything (and your state never leaves
//! the app except to the provider's own API).
//!
//! Supported providers: OpenRouter, OpenAI, Anthropic, Google (Gemini), and
//! SpaceXAI (formerly xAI). Each has its own endpoint, auth scheme, and request
//! shape, all normalized here behind a single `Provider` type.
//!
//! Uses the system `curl` for the HTTPS call so we pull in no TLS/async deps.

use anyhow::{Context, Result};
use std::process::Command;

/// Hard ceiling on generated tokens per request. Keeps a misbehaving model
/// (or a chatty prompt) from running away and torching your provider's
/// tokens-per-minute quota. The local parser only needs a handful of short
/// command lines, and the coach reply is 1-3 sentences, so 1024 is plenty.
const MAX_TOKENS: u32 = 1024;

const COMMAND_SYSTEM: &str = "You are the Pomocard agent, a Pomodoro + Kanban CLI. \
Translate the user's request into one or more Pomocard natural-language commands, \
one per line, with NO extra text, numbering, or markdown. Each line must be a \
command the local parser understands, e.g.: \
add \"Write the spec\" to today; start a 25m focus block; move \"QA pass\" to in progress; \
finish \"Draft chapter\"; pin \"Refactor\"; delete \"Old task\"; estimate \"Big task\" 3; \
stats; clear done; open board; set focus 50; theme dark. \
Use the exact card titles given (quoted). Only output the command lines.";

const COACH_SYSTEM: &str = "You are the Pomocard AI Coach, a friendly Pomodoro + Kanban \
assistant living inside a terminal CLI. The user is talking to you directly. Reply in a \
short, practical, conversational voice (1-3 sentences, no markdown, no code fences, no \
bullet lists). Be encouraging and specific to focus and productivity. Do not invent \
commands unless the user explicitly asks for one.";

/// Which upstream API a request is routed to. Parsed from the `provider`
/// setting; anything unknown falls back to OpenRouter for backwards compat.
pub enum Provider {
    OpenRouter,
    OpenAI,
    Anthropic,
    Google,
    SpaceXAI,
}

impl Provider {
    pub fn parse(s: &str) -> Provider {
        match s.trim().to_lowercase().as_str() {
            "openai" => Provider::OpenAI,
            "anthropic" | "claude" => Provider::Anthropic,
            "google" | "gemini" => Provider::Google,
            "xai" | "spacexai" | "grok" | "spacex" => Provider::SpaceXAI,
            _ => Provider::OpenRouter,
        }
    }

    /// A sensible zero-config model id per provider.
    pub fn default_model(&self) -> &'static str {
        match self {
            Provider::OpenRouter => "qwen/qwen3-8b:free",
            Provider::OpenAI => "gpt-4o-mini",
            Provider::Anthropic => "claude-3-5-haiku-latest",
            Provider::Google => "gemini-2.0-flash",
            Provider::SpaceXAI => "grok-3-mini",
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Provider::OpenRouter => "OpenRouter",
            Provider::OpenAI => "OpenAI",
            Provider::Anthropic => "Anthropic",
            Provider::Google => "Google",
            Provider::SpaceXAI => "SpaceXAI",
        }
    }

    /// Build the request: returns `(url, json_body, extra_headers)`.
    fn build(
        &self,
        model: &str,
        system: &str,
        user: &str,
        temperature: f32,
    ) -> (String, String, Vec<(String, String)>) {
        // OpenRouter keeps the full provider/model id; the direct providers want a
        // bare model id, so drop any provider/ prefix (otherwise, for Google, the
        // slash corrupts the request URL path).
        let bare = model.rsplit('/').next().unwrap_or(model);
        match self {
            Provider::OpenRouter | Provider::OpenAI | Provider::SpaceXAI => {
                let endpoint = match self {
                    Provider::OpenRouter => "https://openrouter.ai/api/v1/chat/completions",
                    Provider::OpenAI => "https://api.openai.com/v1/chat/completions",
                    Provider::SpaceXAI => "https://api.x.ai/v1/chat/completions",
                    _ => unreachable!(),
                };
                let m = match self {
                    Provider::OpenRouter => model,
                    _ => bare,
                };
                let body = serde_json::json!({
                    "model": m,
                    "messages": [
                        { "role": "system", "content": system },
                        { "role": "user", "content": user }
                    ],
                    "temperature": temperature,
                    // Cap tokens so a runaway model can't torch the TPM quota.
                    "max_tokens": MAX_TOKENS
                })
                .to_string();
                (endpoint.to_string(), body, Vec::new())
            }
            Provider::Anthropic => {
                let body = serde_json::json!({
                    "model": bare,
                    "system": system,
                    "max_tokens": MAX_TOKENS,
                    "temperature": temperature,
                    "messages": [ { "role": "user", "content": user } ]
                })
                .to_string();
                (
                    "https://api.anthropic.com/v1/messages".to_string(),
                    body,
                    vec![("anthropic-version".to_string(), "2023-06-01".to_string())],
                )
            }
            Provider::Google => {
                let url = format!(
                    "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
                    bare
                );
                let body = serde_json::json!({
                    "systemInstruction": { "parts": [ { "text": system } ] },
                    "contents": [ { "role": "user", "parts": [ { "text": user } ] } ],
                    "generationConfig": {
                        "temperature": temperature,
                        "maxOutputTokens": MAX_TOKENS
                    }
                })
                .to_string();
                (url, body, Vec::new())
            }
        }
    }

    /// Pull the assistant text out of a provider response, surfacing API errors.
    fn parse_response(&self, text: &str) -> Result<String> {
        if text.trim().is_empty() {
            anyhow::bail!(
                "{} returned an empty response (check the API key, model id, and network)",
                self.name()
            );
        }
        let v: serde_json::Value = serde_json::from_str(text)
            .with_context(|| format!("could not parse {} response: {text}", self.name()))?;
        if let Some(err) = v.get("error") {
            let msg = err
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or(text);
            anyhow::bail!("{} API error: {msg}", self.name());
        }
        let out = match self {
            Provider::OpenRouter | Provider::OpenAI | Provider::SpaceXAI => v
                .get("choices")
                .and_then(|c| c.get(0))
                .and_then(|c| c.get("message"))
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_str()),
            Provider::Anthropic => v
                .get("content")
                .and_then(|c| c.get(0))
                .and_then(|c| c.get("text"))
                .and_then(|t| t.as_str()),
            Provider::Google => v
                .get("candidates")
                .and_then(|c| c.get(0))
                .and_then(|c| c.get("content"))
                .and_then(|c| c.get("parts"))
                .and_then(|p| p.get(0))
                .and_then(|p| p.get("text"))
                .and_then(|t| t.as_str()),
        };
        out.filter(|s| !s.trim().is_empty())
            .map(|s| s.to_string())
            .context(format!("{} returned no content", self.name()))
    }
}

    /// Percent-encode a value for a URL query parameter. Google receives its
    /// API key in the query string, so any non-unreserved character must be
    /// encoded or the key is rejected as invalid.
    fn url_encode(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        for b in s.bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    out.push(b as char);
                }
                _ => {
                    out.push('%');
                    out.push(char::from_digit((b >> 4) as u32, 16).unwrap().to_ascii_uppercase());
                    out.push(char::from_digit((b & 0xf) as u32, 16).unwrap().to_ascii_uppercase());
                }
            }
        }
        out
    }

/// Run a completion against the selected provider and return the assistant text.
fn request(
    provider: &Provider,
    key: &str,
    model: &str,
    system: &str,
    user: &str,
    temperature: f32,
) -> Result<String> {
    let (mut url, body, mut headers) = provider.build(model, system, user, temperature);
    let key = key.trim();
    match provider {
        // Google authenticates via a query parameter, not a header.
        Provider::Google => url = format!("{}?key={}", url, url_encode(key)),
        Provider::Anthropic => headers.push(("x-api-key".to_string(), key.to_string())),
        _ => headers.push(("Authorization".to_string(), format!("Bearer {key}"))),
    }

    let mut cmd = Command::new("curl");
    cmd.args(["-s", "-S", "-X", "POST", &url, "-H", "Content-Type: application/json"]);
    for (k, v) in &headers {
        cmd.arg("-H").arg(format!("{k}: {v}"));
    }
    cmd.arg("-d").arg(&body);

    let output = cmd
        .output()
        .context("failed to spawn `curl` — install curl (ships with Windows 10+, macOS, Linux)")?;
    if !output.status.success() {
        anyhow::bail!(
            "{} request failed (curl exit {:?})",
            provider.name(),
            output.status.code()
        );
    }

    let text = String::from_utf8_lossy(&output.stdout);
    provider.parse_response(&text)
}

/// Translate `user` text into Pomocard natural-language command lines.
pub fn complete(key: &str, provider: &str, model: &str, user: &str) -> Result<String> {
    request(
        &Provider::parse(provider),
        key,
        model,
        COMMAND_SYSTEM,
        user,
        0.2,
    )
}

/// Conversational AI Coach reply for the `Ask` branch.
/// Build the effective AI Coach system prompt. A custom prompt (hotswap)
/// fully replaces the base; otherwise an optional persona flavor is appended.
pub fn coach_system(persona: &str, custom: Option<&str>) -> String {
    if let Some(c) = custom {
        let c = c.trim();
        if !c.is_empty() {
            return c.to_string();
        }
    }
    let flavor = match persona.trim().to_lowercase().as_str() {
        "warm" => " You are warm and encouraging, with a little playful energy — celebrate small wins.",
        "cold" => " You are blunt and terse: no filler, no flattery, just the useful answer.",
        "stoic" => " You are calm and measured, focused on the work itself rather than praise.",
        "chaotic" => " You are energetic and a little unpredictable in tone, but always on-point.",
        _ => "",
    };
    if flavor.is_empty() {
        COACH_SYSTEM.to_string()
    } else {
        format!("{COACH_SYSTEM}{flavor}")
    }
}

pub fn chat(
    key: &str,
    provider: &str,
    model: &str,
    user: &str,
    persona: &str,
    custom: Option<&str>,
) -> Result<String> {
    request(
        &Provider::parse(provider),
        key,
        model,
        &coach_system(persona, custom),
        user,
        0.7,
    )
}
