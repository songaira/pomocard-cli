//! pomocard — a monochrome terminal wrapper around the Pomocard focus workspace.
//!
//! Run with no arguments for the full ratatui app; use the subcommands for
//! scriptable, headless access to the same state file.

mod agent;
mod app;
mod data;
mod llm;
mod input;
mod state;
mod theme;
mod timer;
mod ui;

use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use crossterm::event::{self, Event};

use crate::agent::Entry;
use crate::app::App;
use crate::data::Data;
use crate::state::{col_label, tier_label, COLS};

const VERSION: &str = env!("CARGO_PKG_VERSION");

const USAGE: &str = r#"pomocard — Pomodoro + Kanban, in black and white, in your terminal.

USAGE
  pomocard [OPTIONS]                 launch the TUI (agent harness + board)
  pomocard exec <text...>            run agent commands headlessly
  pomocard status                    print today's focus stats
  pomocard board                     print the Kanban board
  pomocard templates                 list the routine templates
  pomocard path                      print the state file location

OPTIONS
  --state <FILE>     state JSON (default: ~/.pomocard/pomocard.v2.json,
                     or $POMOCARD_STATE); the shape matches the browser's
                     localStorage["pomocard.v2"], so you can copy it over
  --data-dir <DIR>   re-read templates.json / team.json / insights.json
  -h, --help         this text
  -V, --version      version

EXAMPLES
  pomocard exec 'start a 25m focus block and drop "Ship the landing page" into Today'
  pomocard exec 'move "Ship the landing page" to in progress'
  pomocard exec 'finish "Final QA pass" and stats'
"#;

struct Args {
    state: Option<PathBuf>,
    data_dir: Option<PathBuf>,
    command: Option<String>,
    rest: Vec<String>,
}

fn parse_args() -> Result<Option<Args>> {
    let mut out = Args {
        state: None,
        data_dir: None,
        command: None,
        rest: Vec::new(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" | "help" if out.command.is_none() => {
                print!("{USAGE}");
                return Ok(None);
            }
            "-V" | "--version" => {
                println!("pomocard {VERSION}");
                return Ok(None);
            }
            "--state" => {
                out.state = Some(PathBuf::from(
                    it.next().context("--state needs a file path")?,
                ));
            }
            "--data-dir" => {
                out.data_dir = Some(PathBuf::from(
                    it.next().context("--data-dir needs a directory")?,
                ));
            }
            other => {
                if out.command.is_none() {
                    out.command = Some(other.to_string());
                } else {
                    out.rest.push(other.to_string());
                }
            }
        }
    }
    Ok(Some(out))
}

fn main() -> Result<()> {
    let Some(args) = parse_args()? else {
        return Ok(());
    };

    let path = args
        .state
        .clone()
        .unwrap_or_else(state::default_state_path);
    let data = match &args.data_dir {
        Some(dir) => Data::from_dir(dir)?,
        None => Data::embedded(),
    };

    let mut app = App::new(path, data)?;

    match args.command.as_deref() {
        None | Some("tui") | Some("run") => run_tui(app),
        Some("exec") | Some("do") | Some("agent") => {
            let line = args.rest.join(" ");
            if line.trim().is_empty() {
                println!("pomocard: nothing to run. Try: pomocard exec 'add \"Write the spec\" to today'");
                return Ok(());
            }
            app.headless = true;
            let produced = app.exec_line(&line);
            print_entries(&produced);
            Ok(())
        }
        Some("status") | Some("stats") => {
            app.headless = true;
            let produced = app.exec_line("stats");
            print_entries(&produced);
            Ok(())
        }
        Some("board") | Some("ls") => {
            print_board(&app);
            Ok(())
        }
        Some("templates") => {
            for t in &app.data.templates {
                println!(
                    "  {:<28} {} cards · {}",
                    t.name,
                    t.cards.len(),
                    t.desc
                );
            }
            Ok(())
        }
        Some("path") => {
            println!("{}", app.path.display());
            Ok(())
        }
        Some(other) => {
            // treat unknown leading words as an agent line: `pomocard start 25m focus`
            let mut line = other.to_string();
            if !args.rest.is_empty() {
                line.push(' ');
                line.push_str(&args.rest.join(" "));
            }
            app.headless = true;
            let produced = app.exec_line(&line);
            print_entries(&produced);
            Ok(())
        }
    }
}

/* ---------------- headless output ---------------- */

fn print_entries(entries: &[Entry]) {
    for entry in entries {
        match entry {
            Entry::You(text) => println!("\n❯ You    {text}"),
            Entry::Agent(text) => println!("◆ Agent  {text}"),
            Entry::Note(text) => println!("· {text}"),
            Entry::Tool(tool) => {
                println!("│ {}  {}", tool.name, tool.status());
                for (k, v) in &tool.rows {
                    println!("│   {:<9} {}", k, v);
                }
            }
        }
    }
    println!();
}

fn print_board(app: &App) {
    println!();
    for col in COLS {
        let cards = app.state.board.column(col);
        println!("  {} ({})", col_label(col), cards.len());
        if cards.is_empty() {
            println!("    —");
        }
        for card in cards {
            let dots: String = (0..4)
                .map(|i| if i < card.est { '●' } else { '○' })
                .collect();
            let pin = if app.state.active_id.as_deref() == Some(card.id.as_str()) {
                "▶"
            } else {
                " "
            };
            println!("    {pin} {:<44} {dots}", card.title);
        }
        println!();
    }
    println!(
        "  {} · {} plan · {}\n",
        app.headline(),
        tier_label(&app.state.tier),
        app.path.display()
    );
}

/* ---------------- tui loop ---------------- */

fn run_tui(mut app: App) -> Result<()> {
    if !io::stdout().is_terminal() {
        println!("pomocard: not a terminal — use `pomocard exec \"...\"` instead.");
        return Ok(());
    }

    let mut terminal = ratatui::init();
    let res = event_loop(&mut terminal, &mut app);
    ratatui::restore();
    app.save();
    res
}

fn event_loop(terminal: &mut ratatui::DefaultTerminal, app: &mut App) -> Result<()> {
    let frame_budget = Duration::from_millis(200);
    let mut last = Instant::now();

    loop {
        terminal.draw(|f| ui::draw(f, app))?;

        if app.bell {
            app.bell = false;
            let mut out = io::stdout();
            let _ = out.write_all(b"\x07");
            let _ = out.flush();
        }

        let timeout = frame_budget
            .checked_sub(last.elapsed())
            .unwrap_or_else(|| Duration::from_millis(0));
        if event::poll(timeout)? {
            match event::read()? {
                Event::Key(key) => input::handle_key(app, key),
                Event::Resize(_, _) => {}
                _ => {}
            }
        }

        if last.elapsed() >= frame_budget {
            app.tick();
            last = Instant::now();
        }

        if app.should_quit {
            app.save();
            return Ok(());
        }
    }
}
