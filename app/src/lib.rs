//! Vetochki with winit + softbuffer: a full-screen scene where a branch is dragged onto a pile of
//! leaves and a read-only box counts the drops. Behavior lives in `vetochki-core`; this file maps
//! window events and a timer onto it and paints on the CPU. Set `VETOCHKI_TRACE=<file>` to log
//! every state change (one [`Game::observe`] line each) when debugging.
//!
//! A library so that `src/main.rs` (desktop) and `android/` (NativeActivity) share it.

mod paint;

use std::{
    fs::File,
    io::Write,
    num::NonZeroU32,
    rc::Rc,
    time::{Duration, Instant},
};

use paint::{Area, Frame, Glyphs, Image, decode_webp, shrink};
use vetochki_core::{self as model, Game, Point, Rect, Target};
use winit::{
    application::ApplicationHandler,
    dpi::{PhysicalPosition, PhysicalSize},
    event::{ElementState, KeyEvent, TouchPhase, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey},
    window::{Fullscreen, Window, WindowId},
};

/// Themed (comctl32 v6, Windows 10) TEdit look — the original ships a Common-Controls 6 manifest.
const EDIT_BORDER: u32 = 0x7A7A7A;
const SELECTION: u32 = 0x0078D7;
/// Form font `Height = -11`: an 11 px em.
const FONT_SIZE: f32 = 11.0;
/// Everything `edit1.Text` can ever hold: the DFM text and the digits of a score.
const GLYPHS: &str = "Счет0123456789-";
const TICK: Duration = Duration::from_millis(model::TIMER_INTERVAL_MS);
/// The screen the original form is laid out for (every control fits in it).
const FORM_SIZE: (f64, f64) = (800.0, 600.0);

/// Physical pixels per logical (96-DPI form) pixel. Desktops use the display scale, like Windows
/// does for the DPI-unaware original. Android screens are too narrow in DPI-scaled pixels for the
/// layout, so there the form is stretched as far as [`FORM_SIZE`] still fits the screen instead.
fn scale_for(size: PhysicalSize<u32>, display_scale: f64) -> f64 {
    if cfg!(target_os = "android") {
        (f64::from(size.width) / FORM_SIZE.0).min(f64::from(size.height) / FORM_SIZE.1)
    } else {
        display_scale
    }
}

/// Only what is drawn every frame; the static pictures live in [`static_layer`].
struct Assets {
    branch: Image,
    glyphs: Glyphs,
}

impl Assets {
    fn load() -> Self {
        // Shrunk once to VCL's DestRect size; the scene never rescales it.
        let br = model::branch_picture_rect(model::BRANCH_HOME);
        Self {
            branch: shrink(
                &decode_webp(include_bytes!("../assets/branch.webp")),
                br.w as usize,
                br.h as usize,
            ),
            glyphs: Glyphs::new(load_font(), FONT_SIZE, GLYPHS),
        }
    }
}

/// The original draws with Tahoma (the form font). Use the system copy where one exists
/// (Windows, macOS); otherwise an embedded DejaVu Sans subset holding exactly the glyphs the box
/// can show: "Счет" and the digits.
fn load_font() -> fontdue::Font {
    let system = [
        std::env::var_os("WINDIR")
            .map(|w| std::path::PathBuf::from(w).join("Fonts").join("tahoma.ttf")),
        Some("/System/Library/Fonts/Supplemental/Tahoma.ttf".into()),
    ];
    system
        .into_iter()
        .flatten()
        .filter_map(|p| std::fs::read(p).ok())
        .find_map(|b| fontdue::Font::from_bytes(b, fontdue::FontSettings::default()).ok())
        .unwrap_or_else(|| {
            fontdue::Font::from_bytes(
                &include_bytes!("../assets/DejaVuSans-subset.ttf")[..],
                Default::default(),
            )
            .expect("embedded font")
        })
}

/// Where the background photo goes in a `w`×`h` (logical) window. The original draws it unscaled
/// at (-8, -8), which leaves a strip of form colour right or below on a window larger than
/// 2040×1528. Such a window gets the photo enlarged, aspect kept and still anchored at (-8, -8),
/// just enough to cover it (CSS `background-size: cover`, but never shrunk).
fn background_cover(w: usize, h: usize) -> Rect {
    let bg = model::background_rect();
    let need = |len: usize, at: i32, size: i32| (len as f64 - f64::from(at)) / f64::from(size);
    let k = need(w, bg.x, bg.w).max(need(h, bg.y, bg.h));
    if k <= 1.0 {
        return bg;
    }
    let grow = |size: i32| (f64::from(size) * k).ceil() as i32;
    Rect::new(bg.x, bg.y, grow(bg.w), grow(bg.h))
}

/// Background colour, background photo and leaf pile: none of them ever move. The pictures are
/// decoded here and dropped right after (12+ MB less resident memory); this only runs again when
/// the window size or DPI changes.
fn static_layer(w: usize, h: usize) -> Vec<u32> {
    let mut px = vec![model::FORM_COLOR; w * h];
    let mut f = Frame::new(w, h, &mut px);
    let (bg, lv) = (background_cover(w, h), model::leaves_rect());
    f.blit_scaled(
        &decode_webp(include_bytes!("../assets/background.webp")),
        bg.x,
        bg.y,
        bg.w as usize,
        bg.h as usize,
    );
    let leaves = shrink(
        &decode_webp(include_bytes!("../assets/leaves.webp")),
        lv.w as usize,
        lv.h as usize,
    );
    f.blit(&leaves, lv.x, lv.y);
    px
}

/// What the moving part of the frame shows. Redraws are requested, and limited to the affected
/// areas, by comparing this with what is on screen.
#[derive(Clone, PartialEq, Eq)]
struct Shown {
    branch: Point,
    score: String,
    selected: bool,
}

fn branch_area(branch: Point, bounds: Area) -> Option<Area> {
    let r = model::branch_picture_rect(branch);
    Area::clipped(r.x, r.y, r.w as usize, r.h as usize, bounds)
}

fn edit_area(bounds: Area) -> Option<Area> {
    let e = model::EDIT;
    Area::clipped(e.x, e.y, e.w as usize, e.h as usize, bounds)
}

/// Everything above the static layer, limited to `f.clip`. label1 needs nothing: it is painted
/// first in the original and the opaque background covers it.
fn compose(f: &mut Frame, a: &Assets, s: &Shown) {
    let br = model::branch_picture_rect(s.branch);
    f.blit(&a.branch, br.x, br.y);

    let e = model::EDIT;
    f.fill(e.x, e.y, e.w as usize, e.h as usize, EDIT_BORDER);
    f.fill(
        e.x + 1,
        e.y + 1,
        (e.w - 2) as usize,
        (e.h - 2) as usize,
        0xFFFFFF,
    );
    let text_h = (a.glyphs.ascent - a.glyphs.descent).round() as i32;
    let baseline = e.y + 1 + (e.h - 2 - text_h) / 2 + a.glyphs.ascent.round() as i32;
    let x = e.x + 3; // 1 px border + edit margin
    let color = if s.selected {
        let w = a.glyphs.width(&s.score);
        f.fill(
            x - 1,
            e.y + 3,
            (w + 2) as usize,
            (e.h - 6) as usize,
            SELECTION,
        );
        0xFFFFFF
    } else {
        0x000000
    };
    f.text(&a.glyphs, &s.score, x, baseline, color);
}

/// Bring the logical frame up to date for `want`, given that it currently shows `old` (`None`:
/// unknown): restore each changed area from the static layer and recompose only that area.
/// Returns the logical areas that changed.
fn update_frame(
    layer: &[u32],
    frame: &mut [u32],
    (lw, lh): (usize, usize),
    old: Option<&Shown>,
    want: &Shown,
    assets: &Assets,
) -> Vec<Area> {
    let full = Area::full(lw, lh);
    let dirty = match old {
        None => vec![full],
        Some(old) => {
            let mut d = Vec::new();
            if old.branch != want.branch {
                d.extend(branch_area(old.branch, full));
                d.extend(branch_area(want.branch, full));
            }
            if (&old.score, old.selected) != (&want.score, want.selected) {
                d.extend(edit_area(full));
            }
            d
        }
    };
    for a in &dirty {
        for y in a.y0..a.y1 {
            let row = y * lw + a.x0..y * lw + a.x1;
            frame[row.clone()].copy_from_slice(&layer[row]);
        }
        let mut f = Frame::new(lw, lh, frame);
        f.clip = *a;
        compose(&mut f, assets, want);
    }
    dirty
}

/// Nearest-neighbour source of physical pixel `p` — how Windows stretches a DPI-unaware window.
fn to_logical(p: usize, s: f64, logical_len: usize) -> usize {
    ((p as f64 / s) as usize).min(logical_len - 1)
}

/// Physical pixels `[p0, p1)` whose logical source lies in `[l0, l1)`, found with the same
/// mapping as the stretch itself, so floating-point rounding can never leave a stale pixel.
fn physical_span(
    l0: usize,
    l1: usize,
    s: f64,
    logical_len: usize,
    physical_len: usize,
) -> (usize, usize) {
    let first = |l: usize| {
        let mut p = ((l as f64 * s) as usize)
            .saturating_sub(2)
            .min(physical_len);
        while p < physical_len && to_logical(p, s, logical_len) < l {
            p += 1;
        }
        p
    };
    (first(l0), first(l1))
}

fn damage_rect(a: Area) -> Option<softbuffer::Rect> {
    let (width, height) = (
        NonZeroU32::new((a.x1 - a.x0) as u32)?,
        NonZeroU32::new((a.y1 - a.y0) as u32)?,
    );
    Some(softbuffer::Rect {
        x: a.x0 as u32,
        y: a.y0 as u32,
        width,
        height,
    })
}

/// Copy `areas` of the logical frame into the window buffer (stretched unless at 96 DPI) and
/// return the damaged physical rectangles for `present_with_damage`.
fn copy_to_window(
    frame: &[u32],
    (lw, lh): (usize, usize),
    buffer: &mut [u32],
    (pw, ph): (usize, usize),
    s: f64,
    areas: &[Area],
) -> Vec<softbuffer::Rect> {
    let mut damage = Vec::with_capacity(areas.len());
    for a in areas {
        let (x0, x1) = physical_span(a.x0, a.x1, s, lw, pw);
        let (y0, y1) = physical_span(a.y0, a.y1, s, lh, ph);
        for y in y0..y1 {
            let dst = &mut buffer[y * pw + x0..y * pw + x1];
            if s == 1.0 {
                dst.copy_from_slice(&frame[y * lw + x0..y * lw + x1]);
            } else {
                let src = &frame[to_logical(y, s, lh) * lw..][..lw];
                for (x, d) in (x0..x1).zip(dst) {
                    *d = src[to_logical(x, s, lw)];
                }
            }
        }
        damage.extend(damage_rect(Area { x0, y0, x1, y1 }));
    }
    damage
}

struct Gfx {
    window: Rc<Window>,
    _context: softbuffer::Context<Rc<Window>>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
}

struct App {
    assets: Assets,
    game: Game,
    /// The press landed on image1; the release goes back to it (VCL mouse capture).
    captured: bool,
    /// edit1 is the only tab stop, gets focus on show and `AutoSelect` selects its text until the
    /// first `Text` assignment or a click into it. [likely, not verified on the original]
    edit_selected: bool,
    focused: bool,
    /// Last cursor position, client area, physical pixels.
    cursor: PhysicalPosition<f64>,
    /// The finger acting as the mouse (Android): the first one down, until it lifts.
    touch: Option<u64>,
    modifiers: ModifiersState,
    /// Next `timer1.OnTimer`; `None` while the timer is disabled.
    next_tick: Option<Instant>,
    /// Static layer and, when the window is not at 96 DPI, the composed frame to be stretched;
    /// both at 96-DPI logical size. At 100 % the window buffer itself holds the frame.
    layer: Vec<u32>,
    frame: Vec<u32>,
    /// Logical width, height and the scale factor's bits the two were built for.
    logical: (usize, usize, u64),
    surface_size: (u32, u32),
    /// What `frame` and the window currently show; `None` forces a full repaint.
    shown: Option<Shown>,
    trace: Option<File>,
    last_trace: String,
    gfx: Option<Gfx>,
}

impl App {
    fn new() -> Self {
        let mut app = Self {
            assets: Assets::load(),
            game: Game::default(),
            captured: false,
            edit_selected: true,
            focused: true,
            cursor: PhysicalPosition::new(0.0, 0.0),
            touch: None,
            modifiers: ModifiersState::empty(),
            next_tick: None,
            layer: Vec::new(),
            frame: Vec::new(),
            logical: (0, 0, 0),
            surface_size: (0, 0),
            shown: None,
            trace: std::env::var_os("VETOCHKI_TRACE").and_then(|p| File::create(p).ok()),
            last_trace: String::new(),
            gfx: None,
        };
        app.record();
        app
    }

    fn scale(&self) -> f64 {
        self.gfx.as_ref().map_or(1.0, |g| {
            scale_for(g.window.inner_size(), g.window.scale_factor())
        })
    }

    /// onmousedown: on image1 it starts a drag (any button), on edit1 it drops the selection.
    fn press(&mut self, now: Instant) {
        match model::hit_test(self.game.branch, self.client_point()) {
            Some(Target::Branch) => {
                self.captured = true;
                self.game.mouse_down();
            }
            Some(Target::Edit) => self.edit_selected = false,
            None => {}
        }
        self.changed(now);
    }

    /// onmouseup, delivered only while image1 holds the capture.
    fn release(&mut self, now: Instant) {
        if self.captured {
            self.captured = false;
            self.game.mouse_up();
            self.changed(now);
        }
    }

    /// Cursor in form coordinates at 96 DPI (what VCL hit-tests against).
    fn client_point(&self) -> Point {
        let s = self.scale();
        Point::new(
            (self.cursor.x / s).floor() as i32,
            (self.cursor.y / s).floor() as i32,
        )
    }

    /// `cursor_pos_x()`/`cursor_pos_y()`: GetCursorPos — screen, not form, coordinates.
    fn screen_point(&self) -> Point {
        let s = self.scale();
        let origin = self
            .gfx
            .as_ref()
            .and_then(|g| g.window.inner_position().ok())
            .unwrap_or_default();
        Point::new(
            ((f64::from(origin.x) + self.cursor.x) / s).floor() as i32,
            ((f64::from(origin.y) + self.cursor.y) / s).floor() as i32,
        )
    }

    fn record(&mut self) {
        let line = self.game.observe();
        if line != self.last_trace {
            if let Some(f) = &mut self.trace {
                let _ = writeln!(f, "{line}").and_then(|()| f.flush());
            }
            self.last_trace = line;
        }
    }

    fn visible(&self) -> Shown {
        Shown {
            branch: self.game.branch,
            score: self.game.score.clone(),
            selected: self.edit_selected && self.focused,
        }
    }

    /// After any state change: keep the timer in step with `timer1.enable`, trace, and repaint
    /// if something visible changed (a tick with the cursor at rest changes nothing).
    /// Enabling the timer starts a fresh period (VCL `TTimer` re-arms `SetTimer`).
    fn changed(&mut self, now: Instant) {
        match (self.game.timer, self.next_tick) {
            (true, None) => self.next_tick = Some(now + TICK),
            (false, Some(_)) => self.next_tick = None,
            _ => {}
        }
        self.record();
        if let Some(g) = &self.gfx
            && self.shown.as_ref() != Some(&self.visible())
        {
            g.window.request_redraw();
        }
    }

    fn tick(&mut self, due: Instant, now: Instant) {
        let before = self.game.score.clone();
        self.game.tick(self.screen_point());
        if self.game.score != before {
            self.edit_selected = false; // assigning Text drops the AutoSelect selection
        }
        // WM_TIMER never queues up: after a stall there is one tick, not a burst.
        self.next_tick = self
            .game
            .timer
            .then(|| (due + TICK).max(now + Duration::from_millis(1)));
        self.changed(now);
    }

    fn draw(&mut self) {
        let want = self.visible();
        let Self {
            gfx: Some(gfx),
            assets,
            layer,
            frame,
            logical,
            surface_size,
            shown,
            ..
        } = self
        else {
            return;
        };
        let size = gfx.window.inner_size();
        let (Some(nw), Some(nh)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        else {
            return;
        };
        if *surface_size != (size.width, size.height) {
            let _ = gfx.surface.resize(nw, nh);
            *surface_size = (size.width, size.height);
            *shown = None;
        }
        let (pw, ph) = (size.width as usize, size.height as usize);
        // The original is not DPI aware: Windows renders it at 96 DPI and stretches the bitmap.
        // Compose at that logical size and stretch the same way (nearest neighbour).
        let s = scale_for(size, gfx.window.scale_factor());
        let (lw, lh) = (
            (pw as f64 / s).ceil() as usize,
            (ph as f64 / s).ceil() as usize,
        );
        if *logical != (lw, lh, s.to_bits()) {
            *layer = static_layer(lw, lh);
            *frame = if s == 1.0 { Vec::new() } else { layer.clone() };
            *logical = (lw, lh, s.to_bits());
            *shown = None;
        }

        // Repaint only what changed, then show only that. A redraw with nothing changed is the
        // system asking for a repaint, and a buffer that lost the previous frame needs everything.
        let Ok(mut buffer) = gfx.surface.buffer_mut() else {
            return;
        };
        let damage = if s == 1.0 {
            let old = shown.as_ref().filter(|_| buffer.age() == 1);
            let mut dirty = update_frame(layer, &mut buffer[..], (lw, lh), old, &want, assets);
            if dirty.is_empty() {
                dirty = vec![Area::full(lw, lh)];
            }
            dirty.into_iter().filter_map(damage_rect).collect()
        } else {
            let mut dirty = update_frame(layer, frame, (lw, lh), shown.as_ref(), &want, assets);
            if dirty.is_empty() || buffer.age() != 1 {
                dirty = vec![Area::full(lw, lh)];
            }
            copy_to_window(frame, (lw, lh), &mut buffer, (pw, ph), s, &dirty)
        };
        let _ = buffer.present_with_damage(&damage);
        *shown = Some(want);
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        // CFG formsInfo: bsNone + wsMaximized → a borderless window covering the whole monitor.
        let attrs = Window::default_attributes()
            .with_title(model::FORM_CAPTION)
            .with_decorations(false)
            .with_resizable(false)
            .with_fullscreen(Some(Fullscreen::Borderless(None)));
        let window = Rc::new(event_loop.create_window(attrs).expect("create window"));
        let context = softbuffer::Context::new(window.clone()).expect("softbuffer context");
        let surface =
            softbuffer::Surface::new(&context, window.clone()).expect("softbuffer surface");
        window.request_redraw();
        self.gfx = Some(Gfx {
            window,
            _context: context,
            surface,
        });
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let now = Instant::now();
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => self.draw(),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(g) = &self.gfx {
                    g.window.request_redraw();
                }
            }
            WindowEvent::Focused(f) => {
                self.focused = f;
                // On activation changes Windows paints a title strip over the top of the client
                // area (this window has no caption style and no non-client area) without asking
                // us to repaint; a full repaint covers it.
                self.shown = None;
                self.changed(now);
            }
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            // bsNone form: the only way out is Alt+F4, which closes the main form and the app.
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(KeyCode::F4),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } if self.modifiers.alt_key() => event_loop.exit(),
            // Android has no Alt+F4: the system Back key closes the app instead. winit marks every
            // key as handled, so without this Back would do nothing.
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key: Key::Named(NamedKey::BrowserBack),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } if cfg!(target_os = "android") => event_loop.exit(),
            WindowEvent::CursorMoved { position, .. } => self.cursor = position,
            // onmousedown/onmouseup ignore which button it is.
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                ..
            } => self.press(now),
            WindowEvent::MouseInput {
                state: ElementState::Released,
                ..
            } => self.release(now),
            // Android touch: the first finger is the mouse, later ones are ignored. Desktop
            // touch screens already get mouse events from the OS (as the original did).
            WindowEvent::Touch(t) if cfg!(target_os = "android") => match t.phase {
                TouchPhase::Started if self.touch.is_none() => {
                    self.touch = Some(t.id);
                    self.cursor = t.location;
                    self.press(now);
                }
                TouchPhase::Moved if self.touch == Some(t.id) => self.cursor = t.location,
                TouchPhase::Ended | TouchPhase::Cancelled if self.touch == Some(t.id) => {
                    self.touch = None;
                    self.cursor = t.location;
                    self.release(now);
                }
                _ => {}
            },
            _ => {}
        }
    }

    /// Android: the window surface is gone until the next `resumed`, which recreates it. A finger
    /// lifted while the app was in the background never reports it, so the drag ends here.
    fn suspended(&mut self, _: &ActiveEventLoop) {
        self.touch = None;
        self.release(Instant::now());
        self.gfx = None;
        self.surface_size = (0, 0);
        self.shown = None;
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if let Some(due) = self.next_tick
            && now >= due
        {
            self.tick(due, now);
        }
        event_loop.set_control_flow(
            self.next_tick
                .map_or(ControlFlow::Wait, ControlFlow::WaitUntil),
        );
    }
}

pub fn run(event_loop: EventLoop<()>) {
    event_loop.run_app(&mut App::new()).expect("event loop");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_covers_large_windows() {
        // Up to 2040×1528 the photo is drawn exactly as in the original.
        for (w, h) in [(800, 600), (1920, 1080), (2040, 1528)] {
            assert_eq!(background_cover(w, h), model::background_rect());
        }
        for (w, h) in [(2560, 1440), (3840, 2160), (1000, 3000), (2041, 600)] {
            let r = background_cover(w, h);
            assert_eq!((r.x, r.y), (-8, -8), "anchored like the original");
            assert!(
                r.x + r.w >= w as i32 && r.y + r.h >= h as i32,
                "{w}x{h} covered"
            );
            assert!((r.w * 3 - r.h * 4).abs() <= 4, "4:3 kept: {}x{}", r.w, r.h);
        }
    }

    /// Repainting only the changed areas must leave exactly the frame and window buffer that a
    /// full repaint gives — at 100 % and at fractional DPI scales.
    #[test]
    fn incremental_redraw_matches_full_redraw() {
        let assets = Assets::load();
        let state = |x, y, score: &str, selected| Shown {
            branch: Point::new(x, y),
            score: score.into(),
            selected,
        };
        let steps = [
            state(584, 136, "Счет", true),
            state(589, 131, "Счет", true), // small drag: old and new areas overlap
            state(150, 20, "Счет", true),  // jump onto edit1
            state(-120, -60, "Счет", false), // partly off-screen, selection gone
            state(584, 136, "1", false),   // reset + score
            state(2000, 2000, "1", false), // completely off-screen
            state(584, 136, "12", false),
        ];
        let (pw, ph) = (1100, 640);
        for s in [1.0, 1.25, 1.5, 1.75] {
            let (lw, lh) = (
                (pw as f64 / s).ceil() as usize,
                (ph as f64 / s).ceil() as usize,
            );
            let layer = static_layer(lw, lh);
            let (mut frame, mut buffer) = (layer.clone(), vec![0; pw * ph]);
            let mut old = None;
            for want in &steps {
                let areas = update_frame(&layer, &mut frame, (lw, lh), old, want, &assets);
                copy_to_window(&frame, (lw, lh), &mut buffer, (pw, ph), s, &areas);
                let (mut full_frame, mut full_buffer) = (layer.clone(), vec![0; pw * ph]);
                let all = update_frame(&layer, &mut full_frame, (lw, lh), None, want, &assets);
                copy_to_window(&full_frame, (lw, lh), &mut full_buffer, (pw, ph), s, &all);
                assert!(
                    frame == full_frame,
                    "frame differs at scale {s}, score {}",
                    want.score
                );
                assert!(
                    buffer == full_buffer,
                    "window differs at scale {s}, score {}",
                    want.score
                );
                old = Some(want);
            }
        }
    }
}
