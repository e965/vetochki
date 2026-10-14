//! Vetochki — the observable behavior of `SP.exe` (the payload of `Vetochki.exe`), recovered
//! from its SoulEngine blocks and re-expressed without PHP, SoulEngine or the WinRAR SFX.
//!
//! Sources of every constant are cited as `DFM` (Form1.dfm inside `F\XFORMS`), `CFG`
//! (`X_CONFIG`), `PARAMS` (igbinary component params in `HelpKeyword`) or `Lnn` (line of the
//! handler source in `_EXEVFILE`, reproduced in `docs/Static-Analysis.md` §4.5).
//!
//! This crate has no GUI dependency, so the state machine is tested on any host (see
//! [`scenario`]).

pub mod scenario;

use std::cmp::Ordering;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    /// Win32 `PtInRect`: left/top inclusive, right/bottom exclusive.
    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.x < self.x + self.w && p.y >= self.y && p.y < self.y + self.h
    }

    pub fn offset(self, by: Point) -> Self {
        Self::new(self.x + by.x, self.y + by.y, self.w, self.h)
    }
}

// ---------------------------------------------------------------------------------------------
// Form layout. All coordinates are form client pixels at 96 DPI (the original is not DPI aware).
// ---------------------------------------------------------------------------------------------

/// DFM `fmEdit.Caption`. Window text of the main form (the misspelling is original).
pub const FORM_CAPTION: &str = "Hello Word";
/// CFG `apptitle`: Application.Title, shown on the taskbar button of a classic Delphi app.
pub const APP_TITLE: &str = "SP";
/// DFM `fmEdit.Color = 15132390` = `$00E6E6E6` (BGR) → RGB `#E6E6E6`.
pub const FORM_COLOR: u32 = 0xE6E6E6;

/// DFM `image3` (background, `Mosaic`, `Proportional`), picture 2048×1536 RGB.
pub const BACKGROUND: Rect = Rect::new(-8, -8, 2048, 1536);
pub const BACKGROUND_PICTURE: (i32, i32) = (2048, 1536);
/// DFM `image2` (leaf pile, `Center`, `Proportional`), picture 800×348 RGBA.
pub const LEAVES: Rect = Rect::new(72, 120, 328, 200);
pub const LEAVES_PICTURE: (i32, i32) = (800, 348);
/// DFM `image1` (the draggable branch, `Center`, `Proportional`), picture 640×485 RGBA.
pub const BRANCH_HOME: Point = Point::new(584, 136);
pub const BRANCH_SIZE: (i32, i32) = (208, 144);
pub const BRANCH_PICTURE: (i32, i32) = (640, 485);
/// DFM `edit1`: read-only TEdit that ends up showing the score.
pub const EDIT: Rect = Rect::new(112, 40, 104, 21);
/// DFM `label1`. It is created before `image3`, and TLabel/TImage are both graphic controls
/// painted in creation order, so the opaque background always covers it: it is never visible
/// and only serves as a state variable for the timer handler.
pub const LABEL: Rect = Rect::new(624, 16, 80, 24);
/// DFM `label1.Caption` / `edit1.Text` (`#1057#1095#1077#1090`).
pub const INITIAL_TEXT: &str = "Счет";

/// PARAMS `timer1`: `interval = '10'`, `repeat = true`, `enable = false`.
pub const TIMER_INTERVAL_MS: u64 = 10;
/// L30–31: `cursor_pos_x() - 104`, `cursor_pos_y() - 72` (= half of image1's 208×144),
/// i.e. the branch is centred on the cursor while dragged.
pub const GRAB_OFFSET: Point = Point::new(104, 72);
/// L32: the drop zone test `x < 150 && y < 150` on image1's top-left corner.
pub const ZONE_LIMIT: i32 = 150;

/// VCL `TImage.DestRect` with `Stretch = False`: where the picture lands inside the control.
/// `Proportional` only ever shrinks (a picture smaller than the control keeps its size).
pub fn dest_rect(
    picture: (i32, i32),
    control: (i32, i32),
    proportional: bool,
    center: bool,
) -> Rect {
    let (mut w, mut h) = picture;
    let (cw, ch) = control;
    if proportional && (w > cw || h > ch) && w > 0 && h > 0 {
        let aspect = w as f64 / h as f64;
        if w > h {
            w = cw;
            h = (cw as f64 / aspect) as i32; // Trunc
            if h > ch {
                h = ch;
                w = (ch as f64 * aspect) as i32;
            }
        } else {
            h = ch;
            w = (ch as f64 * aspect) as i32;
            if w > cw {
                w = cw;
                h = (cw as f64 / aspect) as i32;
            }
        }
    }
    let (x, y) = if center {
        ((cw - w) / 2, (ch - h) / 2)
    } else {
        (0, 0)
    };
    Rect::new(x, y, w, h)
}

/// Form-space rectangle the background picture is drawn into. `Mosaic` (tiling) has no visible
/// effect because the picture is exactly the control's size.
pub fn background_rect() -> Rect {
    dest_rect(
        BACKGROUND_PICTURE,
        (BACKGROUND.w, BACKGROUND.h),
        true,
        false,
    )
    .offset(Point::new(BACKGROUND.x, BACKGROUND.y))
}

pub fn leaves_rect() -> Rect {
    dest_rect(LEAVES_PICTURE, (LEAVES.w, LEAVES.h), true, true)
        .offset(Point::new(LEAVES.x, LEAVES.y))
}

/// Form-space rectangle of the branch picture for a given image1 position.
pub fn branch_picture_rect(branch: Point) -> Rect {
    dest_rect(BRANCH_PICTURE, BRANCH_SIZE, true, true).offset(branch)
}

// ---------------------------------------------------------------------------------------------
// PHP 5.3 value semantics. The handlers store their state in control properties (strings) and
// rely on PHP's loose arithmetic and comparison, so those rules are part of the behavior.
// ---------------------------------------------------------------------------------------------

const PHP_WS: &[u8] = b" \t\n\r\x0b\x0c";

fn skip_ws(s: &[u8]) -> &[u8] {
    let n = s.iter().take_while(|c| PHP_WS.contains(c)).count();
    &s[n..]
}

/// `is_numeric_string(.., allow_errors = 0)`: the whole string is a decimal number
/// (leading whitespace allowed). Hex strings, which PHP 5.3 also accepts, cannot occur here.
fn php_numeric(s: &[u8]) -> Option<f64> {
    let t = skip_ws(s);
    let body = t
        .strip_prefix(b"-")
        .or_else(|| t.strip_prefix(b"+"))
        .unwrap_or(t);
    let digits = |b: &[u8]| b.iter().take_while(|c| c.is_ascii_digit()).count();
    let int = digits(body);
    let mut i = int;
    let mut frac = 0;
    if body.get(i) == Some(&b'.') {
        frac = digits(&body[i + 1..]);
        i += 1 + frac;
    }
    if int + frac == 0 {
        return None;
    }
    if matches!(body.get(i), Some(b'e' | b'E')) {
        let e = &body[i + 1..];
        let e = e
            .strip_prefix(b"-")
            .or_else(|| e.strip_prefix(b"+"))
            .unwrap_or(e);
        let n = digits(e);
        if n > 0 {
            i = body.len() - e.len() + n;
        }
    }
    (i == body.len())
        .then(|| std::str::from_utf8(t).ok()?.parse().ok())
        .flatten()
}

/// `zendi_smart_strcmp`: numeric strings compare as numbers, anything else byte-wise.
pub fn php_strcmp(a: impl AsRef<[u8]>, b: impl AsRef<[u8]>) -> Ordering {
    let (a, b) = (a.as_ref(), b.as_ref());
    match (php_numeric(a), php_numeric(b)) {
        (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
        _ => a.cmp(b), // zend_binary_strcmp: memcmp of the common prefix, then length
    }
}

/// PHP `$a >= $b` for two strings (compiled as `IS_SMALLER_OR_EQUAL($b, $a)`).
pub fn php_ge(a: impl AsRef<[u8]>, b: impl AsRef<[u8]>) -> bool {
    php_strcmp(b, a) != Ordering::Greater
}

/// PHP `$s += n` on a string: the leading integer prefix is used, a non-numeric string counts
/// as 0 (`"Счет" + 1 == 1`). Score strings are only ever produced by this function, so the
/// float branch of PHP's conversion is not reachable. PHP 5.3 on 32 bit would switch to
/// float formatting past 2^31-1 and to exponent notation past 1e14; i64 matches it up to there.
pub fn php_add(s: &str, n: i64) -> String {
    let t = skip_ws(s.as_bytes());
    let (neg, body) = match t.first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let digits = body
        .iter()
        .take_while(|c| c.is_ascii_digit())
        .fold(0i64, |v, &c| {
            v.saturating_mul(10).saturating_add(i64::from(c - b'0'))
        });
    (if neg { -digits } else { digits })
        .saturating_add(n)
        .to_string()
}

// ---------------------------------------------------------------------------------------------
// The game: the five event handlers of SP.exe as one state machine.
// ---------------------------------------------------------------------------------------------

/// What a mouse press lands on. Only these two controls react to the mouse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// `edit1` — a windowed control, therefore always above every graphic control.
    Edit,
    /// `image1` — topmost graphic control; VCL hit-tests its whole rectangle (alpha is ignored).
    Branch,
}

pub fn hit_test(branch: Point, p: Point) -> Option<Target> {
    if EDIT.contains(p) {
        Some(Target::Edit)
    } else if Rect::new(branch.x, branch.y, BRANCH_SIZE.0, BRANCH_SIZE.1).contains(p) {
        Some(Target::Branch)
    } else {
        None
    }
}

/// Everything the handlers read or write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Game {
    /// `image1.Left/Top`.
    pub branch: Point,
    /// `label1.Caption` — hidden flag: `"Счет"` → `"0"`; `"2"` only inside a single tick.
    pub label: String,
    /// `edit1.Text` — the visible score.
    pub score: String,
    /// `timer1.enable`.
    pub timer: bool,
}

impl Default for Game {
    fn default() -> Self {
        Self {
            branch: BRANCH_HOME,
            label: INITIAL_TEXT.to_owned(),
            score: INITIAL_TEXT.to_owned(),
            timer: false,
        }
    }
}

impl Game {
    /// `___ev_form1_image1::onmousedown` (any button): L11 `timer1->enable = 1`.
    pub fn mouse_down(&mut self) {
        self.timer = true;
    }

    /// `___ev_form1_image1::onmouseup` (any button): L15 `timer1->enable = 0`.
    pub fn mouse_up(&mut self) {
        self.timer = false;
    }

    /// `___ev_form1_timer1::ontimer`, run every [`TIMER_INTERVAL_MS`] while `timer` is set.
    /// `cursor` is the *screen* position (`cursor_pos_x()` = `Mouse.CursorPos` = `GetCursorPos`).
    pub fn tick(&mut self, cursor: Point) {
        // L30–31: follow the cursor.
        self.branch = Point::new(cursor.x - GRAB_OFFSET.x, cursor.y - GRAB_OFFSET.y);
        // L32–35: inside the drop zone → raise the flag.
        if self.branch.x < ZONE_LIMIT && self.branch.y < ZONE_LIMIT {
            self.label = "2".to_owned();
        }
        // L38–45: a separate `if`, not `elseif`. It also fires on the very first tick of the
        // session because `"Счет" >= "2"` holds byte-wise; that free first point is original.
        if php_ge(&self.label, "2") {
            self.branch = BRANCH_HOME;
            self.label = "0".to_owned();
            self.timer = false;
            self.score = php_add(&self.score, 1);
        }
    }

    /// One trace line; the app writes these when `VETOCHKI_TRACE` is set (debugging aid).
    pub fn observe(&self) -> String {
        format!(
            "branch={},{} label={} score={} timer={}",
            self.branch.x,
            self.branch.y,
            self.label,
            self.score,
            u8::from(self.timer)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vcl_dest_rects() {
        assert_eq!(background_rect(), Rect::new(-8, -8, 2048, 1536));
        // 800×348 into 328×200: width-bound, h = Trunc(328 / (800/348)) = 142, centred.
        assert_eq!(leaves_rect(), Rect::new(72, 149, 328, 142));
        // 640×485 into 208×144: height-bound after the "woops" branch, w = 190, centred.
        assert_eq!(
            branch_picture_rect(BRANCH_HOME),
            Rect::new(593, 136, 190, 144)
        );
        // smaller picture is not enlarged by Proportional
        assert_eq!(
            dest_rect((10, 10), (100, 50), true, true),
            Rect::new(45, 20, 10, 10)
        );
    }

    #[test]
    fn php_comparison_quirks() {
        // the initial caption compares greater than "2" in every encoding it can arrive in
        for caption in [&b"\xD1\xF7\xE5\xF2"[..], "Счет".as_bytes(), b"????"] {
            assert!(php_ge(caption, "2"), "{caption:?}");
        }
        assert!(php_ge("2", "2"));
        assert!(!php_ge("0", "2"));
        assert!(php_ge("10", "2"), "numeric strings compare numerically");
        assert!(!php_ge("10", "2a"), "non-numeric operand → byte-wise");
        assert!(php_ge(" 2", "2"), "leading whitespace is numeric");
    }

    #[test]
    fn php_addition() {
        assert_eq!(php_add("Счет", 1), "1");
        assert_eq!(php_add("1", 1), "2");
        assert_eq!(php_add("41", 1), "42");
        assert_eq!(php_add(" 7x", 1), "8");
        assert_eq!(php_add("-3", 1), "-2");
    }

    #[test]
    fn hit_testing() {
        assert_eq!(
            hit_test(BRANCH_HOME, Point::new(584, 136)),
            Some(Target::Branch)
        );
        assert_eq!(
            hit_test(BRANCH_HOME, Point::new(792, 280)),
            None,
            "right/bottom exclusive"
        );
        assert_eq!(
            hit_test(BRANCH_HOME, Point::new(587, 140)),
            Some(Target::Branch),
            "transparent pixels hit too"
        );
        // edit1 wins even where the branch overlaps it
        assert_eq!(
            hit_test(Point::new(150, 0), Point::new(160, 45)),
            Some(Target::Edit)
        );
    }

    #[test]
    fn first_tick_gives_a_free_point() {
        let mut g = Game::default();
        g.mouse_down();
        g.tick(Point::new(700, 200));
        assert_eq!(g.observe(), "branch=584,136 label=0 score=1 timer=0");
    }

    #[test]
    fn drop_in_zone_scores_and_resets() {
        let mut g = Game {
            label: "0".into(),
            score: "1".into(),
            ..Game::default()
        };
        g.mouse_down();
        g.tick(Point::new(400, 300)); // branch at (296, 228): outside the zone
        assert_eq!(g.observe(), "branch=296,228 label=0 score=1 timer=1");
        g.tick(Point::new(253, 221)); // branch at (149, 149): inside
        assert_eq!(g.observe(), "branch=584,136 label=0 score=2 timer=0");
    }

    #[test]
    fn zone_needs_both_axes() {
        let mut g = Game {
            label: "0".into(),
            ..Game::default()
        };
        g.mouse_down();
        g.tick(Point::new(254, 100)); // x = 150 is not < 150
        g.tick(Point::new(100, 222)); // y = 150 is not < 150
        assert_eq!(g.observe(), "branch=-4,150 label=0 score=Счет timer=1");
        g.mouse_up();
        assert!(!g.timer);
    }
}
