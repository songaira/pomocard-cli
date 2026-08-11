//! Seed data shared with the web app.
//!
//! The JSON files in `../data` are baked in at compile time so the binary is
//! standalone, but `--data-dir <path>` re-reads them at runtime if you want to
//! edit templates/team/insights without recompiling.

use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

const TEMPLATES_JSON: &str = include_str!("../data/templates.json");
const TEAM_JSON: &str = include_str!("../data/team.json");
const INSIGHTS_JSON: &str = include_str!("../data/insights.json");

#[derive(Debug, Clone, Deserialize)]
pub struct TemplateCard {
    pub col: String,
    pub title: String,
    #[serde(default = "one")]
    pub est: u8,
}

fn one() -> u8 {
    1
}

#[derive(Debug, Clone, Deserialize)]
pub struct Template {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub desc: String,
    #[serde(default)]
    #[allow(dead_code)] // mirrors the JSON; the web build maps it to an SVG
    pub icon: String,
    #[serde(default)]
    pub cards: Vec<TemplateCard>,
}

#[derive(Debug, Clone, Deserialize)]
struct TemplateFile {
    #[serde(default)]
    templates: Vec<Template>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Member {
    #[serde(default)]
    #[allow(dead_code)] // present in team.json, unused by the CLI surfaces
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub initials: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub online: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Activity {
    pub who: String,
    pub action: String,
    pub target: String,
    #[serde(default)]
    pub to: String,
    #[serde(default)]
    pub ago: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Workload {
    pub name: String,
    pub load: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Team {
    #[serde(default)]
    pub members: Vec<Member>,
    #[serde(default)]
    pub activity: Vec<Activity>,
    #[serde(default)]
    pub workload: Vec<Workload>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Insights {
    #[serde(default)]
    pub insights: Vec<String>,
    #[serde(default, rename = "coachLines")]
    pub coach_lines: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Data {
    pub templates: Vec<Template>,
    pub team: Team,
    pub insights: Insights,
}

pub fn initials(name: &str) -> String {
    let mut out = String::new();
    for part in name.split_whitespace().take(2) {
        if let Some(c) = part.chars().next() {
            out.push(c.to_ascii_uppercase());
        }
    }
    if out.is_empty() {
        out.push('?');
    }
    out
}

impl Data {
    pub fn embedded() -> Data {
        let templates: TemplateFile =
            serde_json::from_str(TEMPLATES_JSON).unwrap_or(TemplateFile { templates: vec![] });
        let team: Team = serde_json::from_str(TEAM_JSON).unwrap_or(Team {
            members: vec![],
            activity: vec![],
            workload: vec![],
        });
        let insights: Insights = serde_json::from_str(INSIGHTS_JSON).unwrap_or(Insights {
            insights: vec![],
            coach_lines: vec![],
        });
        Data {
            templates: templates.templates,
            team,
            insights,
        }
    }

    pub fn from_dir(dir: &Path) -> Result<Data> {
        let read = |name: &str| -> Result<String> {
            let p = dir.join(name);
            std::fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))
        };
        let tf: TemplateFile = serde_json::from_str(&read("templates.json")?)
            .context("parsing templates.json")?;
        let team: Team = serde_json::from_str(&read("team.json")?).context("parsing team.json")?;
        let insights: Insights =
            serde_json::from_str(&read("insights.json")?).context("parsing insights.json")?;
        Ok(Data {
            templates: tf.templates,
            team,
            insights,
        })
    }

    pub fn find_template(&self, query: &str) -> Option<&Template> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return None;
        }
        self.templates
            .iter()
            .find(|t| t.id.to_lowercase() == q || t.name.to_lowercase() == q)
            .or_else(|| {
                self.templates.iter().find(|t| {
                    t.name.to_lowercase().contains(&q)
                        || t.id.to_lowercase().contains(&q)
                        || q.split_whitespace()
                            .any(|w| w.len() > 3 && t.name.to_lowercase().contains(w))
                })
            })
    }

    /// Deterministic pick so the CLI never needs an RNG dependency.
    pub fn coach_line(&self, n: usize) -> String {
        if self.insights.coach_lines.is_empty() {
            return "Ready when you are. Pick a card and hit start.".into();
        }
        self.insights.coach_lines[n % self.insights.coach_lines.len()].clone()
    }
}
