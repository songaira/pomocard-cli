//! Pomodoro engine — a port of the `Timer` object in the web app's app.js.

use std::time::Instant;

use crate::state::Settings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Focus,
    Short,
    Long,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Focus => "Focus",
            Mode::Short => "Short break",
            Mode::Long => "Long break",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            Mode::Focus => "Focus",
            Mode::Short => "Short",
            Mode::Long => "Long",
        }
    }

    pub fn parse(raw: &str) -> Option<Mode> {
        let s = raw.trim().to_lowercase();
        if s.contains("long") {
            Some(Mode::Long)
        } else if s.contains("short") || s == "break" {
            Some(Mode::Short)
        } else if s.contains("focus") || s.contains("deep") || s.contains("work") {
            Some(Mode::Focus)
        } else {
            None
        }
    }
}

pub struct Timer {
    pub mode: Mode,
    pub running: bool,
    pub total: f64,
    pub remaining: f64,
    last: Instant,
}

impl Timer {
    pub fn new(settings: &Settings) -> Timer {
        let total = Timer::dur_for(settings, Mode::Focus);
        Timer {
            mode: Mode::Focus,
            running: false,
            total,
            remaining: total,
            last: Instant::now(),
        }
    }

    pub fn dur_for(settings: &Settings, mode: Mode) -> f64 {
        let mins = match mode {
            Mode::Focus => settings.focus,
            Mode::Short => settings.short,
            Mode::Long => settings.long,
        };
        (mins.max(1) as f64) * 60.0
    }

    pub fn set_mode(&mut self, settings: &Settings, mode: Mode) {
        self.mode = mode;
        self.total = Timer::dur_for(settings, mode);
        self.remaining = self.total;
        self.running = false;
        self.last = Instant::now();
    }

    /// Ad-hoc block length, e.g. `start a 50m focus block`.
    pub fn set_duration(&mut self, minutes: u32) {
        self.total = (minutes.max(1) as f64) * 60.0;
        self.remaining = self.total;
    }

    pub fn start(&mut self) {
        if self.remaining <= 0.0 {
            self.remaining = self.total;
        }
        self.running = true;
        self.last = Instant::now();
    }

    pub fn pause(&mut self) {
        self.running = false;
    }

    pub fn toggle(&mut self) {
        if self.running {
            self.pause();
        } else {
            self.start();
        }
    }

    pub fn reset(&mut self) {
        self.running = false;
        self.remaining = self.total;
        self.last = Instant::now();
    }

    /// Returns true on the tick where the session hits zero.
    pub fn tick(&mut self) -> bool {
        let now = Instant::now();
        if !self.running {
            self.last = now;
            return false;
        }
        let delta = now.duration_since(self.last).as_secs_f64();
        self.last = now;
        self.remaining -= delta;
        if self.remaining <= 0.0 {
            self.remaining = 0.0;
            self.running = false;
            return true;
        }
        false
    }

    /// 1.0 at the start of a session, 0.0 when it ends.
    pub fn progress(&self) -> f64 {
        if self.total <= 0.0 {
            0.0
        } else {
            (self.remaining / self.total).clamp(0.0, 1.0)
        }
    }

    pub fn clock(&self) -> String {
        fmt_clock(self.remaining)
    }
}

pub fn fmt_clock(seconds: f64) -> String {
    let s = seconds.max(0.0).ceil() as u32;
    format!("{:02}:{:02}", s / 60, s % 60)
}
