//! Input scenarios: the reconstructed behavior written as replayable input with expectations,
//! run on the deterministic model by `tests/scenarios.rs`.
//!
//! ```text
//! # comment
//! wait 300                 # let 300 ms pass
//! move 690 210 [400]       # move the cursor to (690, 210), linearly over 400 ms (default: jump)
//! down [left|right|middle] # press (default left)
//! up   [left|right|middle]
//! check name score=1 branch=584,136 label=0 timer=0   # every key is optional
//! ```
//!
//! Coordinates are screen pixels at 96 DPI; the window is full-screen on the primary monitor,
//! so they equal form coordinates. The real timer fires every ~15.6 ms on stock Windows instead
//! of the nominal 10 ms, so scenarios should rest ≥ 100 ms before each `check`.

use crate::{Game, Point, TIMER_INTERVAL_MS, Target, hit_test};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
    Middle,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Wait(u64),
    Move { to: Point, ms: u64 },
    Down(Button),
    Up(Button),
    Check { name: String, expect: Expect },
}

/// Expected values of a `check`; `None` means "not checked".
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Expect {
    pub score: Option<String>,
    pub branch: Option<Point>,
    pub label: Option<String>,
    pub timer: Option<bool>,
}

/// The observable state at a checkpoint (what [`Game::observe`] prints).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Observed {
    pub score: Option<String>,
    pub branch: Option<Point>,
    pub label: Option<String>,
    pub timer: Option<bool>,
}

fn point(v: &str) -> Option<Point> {
    let (x, y) = v.split_once(',')?;
    Some(Point::new(x.trim().parse().ok()?, y.trim().parse().ok()?))
}

fn fields(words: &[&str]) -> Result<Observed, String> {
    let mut o = Observed::default();
    for w in words {
        let (k, v) = w
            .split_once('=')
            .ok_or_else(|| format!("expected key=value, got {w:?}"))?;
        match k {
            "score" => o.score = Some(v.to_owned()),
            "label" => o.label = Some(v.to_owned()),
            "branch" => o.branch = Some(point(v).ok_or_else(|| format!("bad point {v:?}"))?),
            "timer" => o.timer = Some(v == "1"),
            _ => return Err(format!("unknown key {k:?}")),
        }
    }
    Ok(o)
}

impl Observed {
    fn of(g: &Game) -> Self {
        Self {
            score: Some(g.score.clone()),
            branch: Some(g.branch),
            label: Some(g.label.clone()),
            timer: Some(g.timer),
        }
    }

    /// Mismatches between `expect` and what was observed; keys `expect` leaves out are skipped.
    pub fn mismatches(&self, expect: &Expect) -> Vec<String> {
        let mut out = Vec::new();
        let mut cmp = |key: &str, want: Option<String>, got: Option<String>| {
            if let (Some(w), Some(g)) = (want, got)
                && w != g
            {
                out.push(format!("{key}: expected {w}, got {g}"));
            }
        };
        let pt = |p: Option<Point>| p.map(|p| format!("{},{}", p.x, p.y));
        cmp("score", expect.score.clone(), self.score.clone());
        cmp("branch", pt(expect.branch), pt(self.branch));
        cmp("label", expect.label.clone(), self.label.clone());
        cmp(
            "timer",
            expect.timer.map(|t| u8::from(t).to_string()),
            self.timer.map(|t| u8::from(t).to_string()),
        );
        out
    }
}

fn button(arg: Option<&&str>) -> Result<Button, String> {
    match arg.copied() {
        None | Some("left") => Ok(Button::Left),
        Some("right") => Ok(Button::Right),
        Some("middle") => Ok(Button::Middle),
        Some(b) => Err(format!("unknown button {b:?}")),
    }
}

pub fn parse(text: &str) -> Result<Vec<Step>, String> {
    let mut steps = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        let w: Vec<&str> = line.split_whitespace().collect();
        let num = |i: usize| -> Result<i64, String> {
            w.get(i)
                .ok_or("missing number")?
                .parse::<i64>()
                .map_err(|e| e.to_string())
        };
        let step = match w.first().copied() {
            None => continue,
            Some("wait") => num(1).map(|ms| Step::Wait(ms as u64)),
            Some("move") => (|| {
                let to = Point::new(num(1)? as i32, num(2)? as i32);
                let ms = if w.len() > 3 { num(3)? as u64 } else { 0 };
                Ok(Step::Move { to, ms })
            })(),
            Some("down") => button(w.get(1)).map(Step::Down),
            Some("up") => button(w.get(1)).map(Step::Up),
            Some("check") => match w.get(1) {
                Some(name) => fields(&w[2..]).map(|o| Step::Check {
                    name: (*name).to_owned(),
                    expect: Expect {
                        score: o.score,
                        branch: o.branch,
                        label: o.label,
                        timer: o.timer,
                    },
                }),
                None => Err("check needs a name".to_owned()),
            },
            Some(cmd) => Err(format!("unknown command {cmd:?}")),
        };
        steps.push(step.map_err(|e| format!("line {}: {e}", n + 1))?);
    }
    Ok(steps)
}

/// Linear cursor path between two points.
pub fn lerp(from: Point, to: Point, elapsed: u64, total: u64) -> Point {
    if total == 0 || elapsed >= total {
        return to;
    }
    let f = |a: i32, b: i32| a + ((i64::from(b - a) * elapsed as i64) / total as i64) as i32;
    Point::new(f(from.x, to.x), f(from.y, to.y))
}

/// Deterministic replay: 1 ms resolution, timer ticks every [`TIMER_INTERVAL_MS`] starting one
/// interval after it is switched on (VCL `TTimer` re-arms `SetTimer` on every enable).
pub struct Sim {
    pub game: Game,
    pub cursor: Point,
    now: u64,
    next_tick: Option<u64>,
    captured: bool,
}

impl Default for Sim {
    fn default() -> Self {
        Self {
            game: Game::default(),
            cursor: Point::new(0, 0),
            now: 0,
            next_tick: None,
            captured: false,
        }
    }
}

impl Sim {
    fn sync_timer(&mut self) {
        match (self.game.timer, self.next_tick) {
            (true, None) => self.next_tick = Some(self.now + TIMER_INTERVAL_MS),
            (false, Some(_)) => self.next_tick = None,
            _ => {}
        }
    }

    fn advance(&mut self, ms: u64, mut path: impl FnMut(u64) -> Point) {
        for dt in 1..=ms {
            self.now += 1;
            self.cursor = path(dt);
            if self.next_tick == Some(self.now) {
                self.next_tick = None;
                self.game.tick(self.cursor);
                if self.game.timer {
                    self.next_tick = Some(self.now + TIMER_INTERVAL_MS);
                }
            }
        }
    }

    /// Apply one step; returns the observation for `check` steps.
    pub fn step(&mut self, step: &Step) -> Option<Observed> {
        match step {
            Step::Wait(ms) => {
                let c = self.cursor;
                self.advance(*ms, |_| c);
            }
            Step::Move { to, ms } => {
                let (from, to, ms) = (self.cursor, *to, *ms);
                self.advance(ms, |dt| lerp(from, to, dt, ms));
                self.cursor = to;
            }
            // VCL delivers the release to the control that got the press (mouse capture),
            // so `onmouseup` fires even after the branch has snapped back home.
            Step::Down(_) => {
                if hit_test(self.game.branch, self.cursor) == Some(Target::Branch) {
                    self.captured = true;
                    self.game.mouse_down();
                }
            }
            Step::Up(_) => {
                if std::mem::take(&mut self.captured) {
                    self.game.mouse_up();
                }
            }
            Step::Check { .. } => return Some(Observed::of(&self.game)),
        }
        self.sync_timer();
        None
    }
}

/// Replay a scenario and return `(check name, expectation, observation)` triples.
pub fn simulate(steps: &[Step]) -> Vec<(String, Expect, Observed)> {
    let mut sim = Sim::default();
    steps
        .iter()
        .filter_map(|s| {
            let obs = sim.step(s)?;
            let Step::Check { name, expect } = s else {
                unreachable!()
            };
            Some((name.clone(), expect.clone(), obs))
        })
        .collect()
}
