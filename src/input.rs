//! Keyboard handling: vim-ish normal mode + an always-available agent prompt.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::app::{App, InputMode, Palette, View};
use crate::state::COLS;
use crate::timer::Mode;

pub fn handle_key(app: &mut App, key: KeyEvent) {
    if key.kind != KeyEventKind::Press {
        return;
    }
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    if ctrl && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('q')) {
        app.should_quit = true;
        return;
    }

    if app.help {
        app.help = false;
        return;
    }

    if app.palette.is_some() {
        palette_key(app, key, ctrl);
        return;
    }

    if ctrl && matches!(key.code, KeyCode::Char('k') | KeyCode::Char('p')) {
        app.palette = Some(Palette {
            query: String::new(),
            sel: 0,
        });
        return;
    }

    match app.mode {
        InputMode::Prompt => prompt_key(app, key, ctrl),
        InputMode::Normal => normal_key(app, key, ctrl),
    }
}

/* ---------------- palette ---------------- */

fn palette_key(app: &mut App, key: KeyEvent, _ctrl: bool) {
    let items = app.filtered_palette();
    let Some(p) = app.palette.as_mut() else { return };
    match key.code {
        KeyCode::Esc => app.palette = None,
        KeyCode::Enter => {
            let cmd = items.get(p.sel).map(|a| a.cmd.clone());
            app.palette = None;
            if let Some(cmd) = cmd {
                app.exec_line(&cmd);
                app.clamp_selection();
            }
        }
        KeyCode::Up => p.sel = p.sel.saturating_sub(1),
        KeyCode::Down => p.sel = (p.sel + 1).min(items.len().saturating_sub(1)),
        KeyCode::Backspace => {
            p.query.pop();
            p.sel = 0;
        }
        KeyCode::Char(c) => {
            p.query.push(c);
            p.sel = 0;
        }
        _ => {}
    }
}

/* ---------------- prompt ---------------- */

fn prompt_key(app: &mut App, key: KeyEvent, ctrl: bool) {
    match key.code {
        KeyCode::Esc => app.mode = InputMode::Normal,
        KeyCode::Enter => {
            let line = app.input.trim().to_string();
            app.input.clear();
            app.cursor = 0;
            if line.is_empty() {
                return;
            }
            app.history.push(line.clone());
            app.hist_idx = None;
            app.exec_line(&line);
            app.clamp_selection();
        }
        KeyCode::Backspace => {
            if app.cursor > 0 {
                let prev = prev_boundary(&app.input, app.cursor);
                app.input.replace_range(prev..app.cursor, "");
                app.cursor = prev;
            }
        }
        KeyCode::Delete => {
            if app.cursor < app.input.len() {
                let next = next_boundary(&app.input, app.cursor);
                app.input.replace_range(app.cursor..next, "");
            }
        }
        KeyCode::Left => app.cursor = prev_boundary(&app.input, app.cursor),
        KeyCode::Right => app.cursor = next_boundary(&app.input, app.cursor),
        KeyCode::Home => app.cursor = 0,
        KeyCode::End => app.cursor = app.input.len(),
        KeyCode::Up => history_step(app, -1),
        KeyCode::Down => history_step(app, 1),
        KeyCode::Tab => app.cycle_view(1),
        KeyCode::BackTab => app.cycle_view(-1),
        KeyCode::PageUp => scroll(app, -5),
        KeyCode::PageDown => scroll(app, 5),
        KeyCode::Char('u') if ctrl => {
            app.input.clear();
            app.cursor = 0;
        }
        KeyCode::Char('w') if ctrl => {
            let head = &app.input[..app.cursor];
            let trimmed = head.trim_end();
            let cut = trimmed
                .rfind(char::is_whitespace)
                .map(|i| i + 1)
                .unwrap_or(0);
            app.input.replace_range(cut..app.cursor, "");
            app.cursor = cut;
        }
        KeyCode::Char('a') if ctrl => app.cursor = 0,
        KeyCode::Char('e') if ctrl => app.cursor = app.input.len(),
        KeyCode::Char(c) => {
            app.input.insert(app.cursor, c);
            app.cursor += c.len_utf8();
        }
        _ => {}
    }
}

fn history_step(app: &mut App, delta: i32) {
    if app.history.is_empty() {
        return;
    }
    let len = app.history.len();
    let idx = match app.hist_idx {
        None => {
            if delta < 0 {
                len - 1
            } else {
                return;
            }
        }
        Some(i) => {
            let next = i as i32 + delta;
            if next < 0 {
                0
            } else if next as usize >= len {
                app.hist_idx = None;
                app.input.clear();
                app.cursor = 0;
                return;
            } else {
                next as usize
            }
        }
    };
    app.hist_idx = Some(idx);
    app.input = app.history[idx].clone();
    app.cursor = app.input.len();
}

/* ---------------- normal ---------------- */

fn normal_key(app: &mut App, key: KeyEvent, _ctrl: bool) {
    match key.code {
        KeyCode::Char('i') | KeyCode::Char(':') | KeyCode::Enter if app.view != View::Templates => {
            enter_prompt(app)
        }
        KeyCode::Enter if app.view == View::Templates => {
            let name = app
                .data
                .templates
                .get(app.tpl_sel)
                .map(|t| t.name.clone())
                .unwrap_or_default();
            if !name.is_empty() {
                app.exec_line(&format!("load template \"{name}\""));
                app.clamp_selection();
            }
        }
        KeyCode::Char('i') | KeyCode::Char(':') => enter_prompt(app),
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Char('?') => app.help = true,
        KeyCode::Esc => {}
        KeyCode::Tab => app.cycle_view(1),
        KeyCode::BackTab => app.cycle_view(-1),
        KeyCode::Char(c @ '1'..='8') => {
            let idx = c as usize - '1' as usize;
            app.go(View::ALL[idx]);
        }
        KeyCode::Char(' ') => {
            app.timer.toggle();
            app.follow = true;
        }
        KeyCode::Char('r') => app.timer.reset(),
        KeyCode::Char('n') => {
            app.exec_line("skip");
        }
        KeyCode::Char('f') => app.timer.set_mode(&app.state.settings, Mode::Focus),
        KeyCode::Char('s') => app.timer.set_mode(&app.state.settings, Mode::Short),
        KeyCode::Char('l') if app.view != View::Board => {
            app.timer.set_mode(&app.state.settings, Mode::Long)
        }
        KeyCode::Char('T') => {
            app.exec_line("theme toggle");
        }
        KeyCode::PageUp => scroll(app, -5),
        KeyCode::PageDown => scroll(app, 5),
        KeyCode::Char('g') => {
            app.follow = false;
            app.scroll = 0;
        }
        KeyCode::Char('G') => app.follow = true,
        _ => view_key(app, key),
    }
}

fn enter_prompt(app: &mut App) {
    app.mode = InputMode::Prompt;
    app.cursor = app.input.len();
}

fn view_key(app: &mut App, key: KeyEvent) {
    match app.view {
        View::Board => board_key(app, key),
        View::Templates => match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                app.tpl_sel = (app.tpl_sel + 1).min(app.data.templates.len().saturating_sub(1))
            }
            KeyCode::Char('k') | KeyCode::Up => app.tpl_sel = app.tpl_sel.saturating_sub(1),
            _ => {}
        },
        _ => match key.code {
            KeyCode::Char('j') | KeyCode::Down => scroll(app, 1),
            KeyCode::Char('k') | KeyCode::Up => scroll(app, -1),
            _ => {}
        },
    }
}

fn board_key(app: &mut App, key: KeyEvent) {
    let col_count = COLS.len();
    match key.code {
        KeyCode::Left | KeyCode::Char('h') => {
            app.sel_col = app.sel_col.saturating_sub(1);
            app.clamp_selection();
        }
        KeyCode::Right | KeyCode::Char('l') => {
            app.sel_col = (app.sel_col + 1).min(col_count - 1);
            app.clamp_selection();
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let len = app.state.board.column(COLS[app.sel_col]).len();
            if len > 0 {
                app.sel_row = (app.sel_row + 1).min(len - 1);
            }
        }
        KeyCode::Up | KeyCode::Char('k') => app.sel_row = app.sel_row.saturating_sub(1),
        KeyCode::Char('H') => shift_card(app, -1),
        KeyCode::Char('L') => shift_card(app, 1),
        KeyCode::Char('J') => reorder_card(app, 1),
        KeyCode::Char('K') => reorder_card(app, -1),
        KeyCode::Char('a') => {
            app.mode = InputMode::Prompt;
            app.input = format!("add \"\" to {}", COLS[app.sel_col]);
            app.cursor = 5; // between the quotes
        }
        KeyCode::Char('e') => {
            if let Some(title) = selected_title(app) {
                app.mode = InputMode::Prompt;
                app.input = format!("rename \"{title}\" to \"\"");
                app.cursor = app.input.len() - 1;
            }
        }
        KeyCode::Char('x') => {
            if let Some(id) = app.selected_card_id() {
                app.state.delete_card(&id);
                app.clamp_selection();
                app.save();
            }
        }
        KeyCode::Char('c') => {
            if let Some(id) = app.selected_card_id() {
                if let Some((col, i)) = app.state.board.find(&id) {
                    let card = &mut app.state.board.column_mut(col)[i];
                    card.est = if card.est >= 4 { 1 } else { card.est + 1 };
                    app.save();
                }
            }
        }
        KeyCode::Char('p') => {
            if let Some(id) = app.selected_card_id() {
                let already = app.state.active_id.as_deref() == Some(id.as_str());
                app.state.set_active(if already { None } else { Some(&id) });
                app.save();
            }
        }
        KeyCode::Char('d') => {
            if let Some(id) = app.selected_card_id() {
                app.state.move_card(&id, "done", None);
                app.clamp_selection();
                app.save();
            }
        }
        _ => {}
    }
}

fn selected_title(app: &App) -> Option<String> {
    app.state
        .board
        .column(COLS[app.sel_col])
        .get(app.sel_row)
        .map(|c| c.title.clone())
}

fn shift_card(app: &mut App, delta: i32) {
    let Some(id) = app.selected_card_id() else {
        return;
    };
    let target = (app.sel_col as i32 + delta).clamp(0, COLS.len() as i32 - 1) as usize;
    if target == app.sel_col {
        return;
    }
    app.state.move_card(&id, COLS[target], None);
    app.sel_col = target;
    app.sel_row = app.state.board.column(COLS[target]).len().saturating_sub(1);
    app.clamp_selection();
    app.save();
}

fn reorder_card(app: &mut App, delta: i32) {
    let col = COLS[app.sel_col];
    let len = app.state.board.column(col).len();
    if len < 2 {
        return;
    }
    let from = app.sel_row.min(len - 1);
    let to = (from as i32 + delta).clamp(0, len as i32 - 1) as usize;
    if to == from {
        return;
    }
    let cards = app.state.board.column_mut(col);
    let card = cards.remove(from);
    cards.insert(to, card);
    app.sel_row = to;
    app.save();
}

fn scroll(app: &mut App, delta: i32) {
    if delta < 0 {
        app.follow = false;
        app.scroll = app.scroll.saturating_sub((-delta) as u16);
    } else {
        app.scroll = app.scroll.saturating_add(delta as u16);
    }
}

/* ---------------- utf-8 cursor helpers ---------------- */

fn prev_boundary(s: &str, i: usize) -> usize {
    let mut j = i.min(s.len());
    loop {
        if j == 0 {
            return 0;
        }
        j -= 1;
        if s.is_char_boundary(j) {
            return j;
        }
    }
}

fn next_boundary(s: &str, i: usize) -> usize {
    let mut j = i;
    loop {
        if j >= s.len() {
            return s.len();
        }
        j += 1;
        if s.is_char_boundary(j) {
            return j;
        }
    }
}
