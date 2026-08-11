# pomocard-cli

A monochrome [ratatui](https://ratatui.rs) terminal client for the Pomocard
Pomodoro + Kanban workspace. It is a faithful CLI twin of the web app
(`pomocard2`): same state shape, same agent harness, strictly black & white.

## Requirements

- **Rust** (stable). This repo pins `channel = "stable"` via `rust-toolchain.toml`,
  so `rustup` will fetch the right toolchain automatically on first build.
- **A linker**:
  - **Linux / macOS** — ships with the OS toolchain. Just `cargo build`.
  - **Windows** — needs either the **MSVC build tools** (Visual Studio) **or**
    **MinGW** (e.g. WinLibs). If `cargo build` complains about a missing
    `link.exe`, install WinLibs and put its `mingw64\bin` on `PATH`, then use the
    GNU target: `rustup target add stable-x86_64-pc-windows-gnu`.
- No other dependencies: `ratatui`, `crossterm`, `serde`, `chrono`, `anyhow` are
  pulled from crates.io by `cargo`. The data files (`data/*.json`) are bundled in
  the repo, so a clone is self-contained.

## Install (script)

One-liners that download the latest release binary and drop it on your `PATH`
(uses `gh` when available, so it works even on a **private** repo after
`gh auth login`):

```powershell
# Windows (PowerShell)
irm https://raw.githubusercontent.com/songaira/pomocard-cli/main/install.ps1 | iex
```

```sh
# Linux / macOS
curl -fsSL https://raw.githubusercontent.com/songaira/pomocard-cli/main/install.sh | bash
```

> The scripts fetch release assets named `pomocard.exe` (Windows) and
> `pomocard-linux` (Linux). Upload those as assets on a GitHub Release, or build
> from source with `cargo install --path .`.

## Build & run

```powershell
$env:Path = "C:\path\to\mingw64\bin;$env:USERPROFILE\.cargo\bin;$env:Path"
cargo run            # launch the TUI
cargo run -- --help  # subcommands
```

A prebuilt binary is at `target/release/pomocard.exe`.

## Usage

Interactive TUI:

```
pomocard                 # launch
pomocard --state FILE    # custom state file
pomocard --data-dir DIR  # where data/*.json live (defaults to bundled)
```

Headless / scripting (no TUI — great for the agent harness):

```
pomocard exec "start a 25m focus block and drop 'Ship the landing page' into Today"
pomocard status
pomocard board
pomocard templates
pomocard path            # print resolved state file path
```

The `exec` command accepts one or more natural-language clauses separated by
`and` / `then` / `,` and prints a transcript of tool calls. Example:

```
pomocard exec "add 'Write changelog' to Backlog; pin it; start 25"
```

## State

State is stored as JSON compatible with the browser's
`localStorage["pomocard.v2"]`, so you can copy the file in and out of the web
app. Default path: `~/.pomocard/pomocard.v2.json`
(override with `--state` or the `POMOCARD_STATE` env var).

## Views

`Tab` cycles: Agent · Board · Analytics · AI Coach · Templates · Team ·
Billing · Settings. Pro/Team views render a blurred paywall overlay when the
account tier is too low.

## Key bindings

- `Esc` — normal mode
- `: ` or `^K` — command palette / prompt (type natural language or `help`)
- `Tab` / `Shift+Tab` — next / previous view
- `h j k l` / arrows — navigate the board
- `a` add · `x` delete · `c` estimate · `p` pin · `m` move · `Enter` focus
- `Space` start/pause · `r` reset · `n` skip
- `^C` quit

## Agent verbs

`start/begin/resume`, `pause/stop`, `reset`, `skip/next`, `break`,
`add/create/capture/drop/put`, `move/push/send`, `finish/complete/ship`,
`delete/remove`, `pin`, `rename`, `estimate`, `clear done`, `list/board`,
`stats/status/streak`, `template/routine/seed`, `upgrade/subscribe`, `theme`,
`set <key> <value>`, `open <view>`, `sync`, `export`, `help`, `quit`.
