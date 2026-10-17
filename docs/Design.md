# Vetochki — reimplementation design

Native Rust (winit + softbuffer) reimplementation of the original `Vetochki.exe` (not included) **without the WinRAR SFX, PHP or
SoulEngine**. The program's behavior was recovered statically (see [Static-Analysis.md](Static-Analysis.md)); nothing here
was translated from Ghidra pseudocode. The application logic came from decoding the original PHP
bytecode exactly (handler source reproduced in Static-Analysis.md §4.5), and the components'
parameters from the form resources. The analysis tooling (`bcdecode.py`, `dfm_params.py`, the
extracted blocks) is kept outside this repository.

Evidence tags: **[C]** confirmed from code/data · **[L]** likely · **[?]** unknown.

```
core/       vetochki-core   — model: constants, VCL layout math, PHP value rules, state machine,
                              scenario format + deterministic simulator. No dependencies.
app/        vetochki        — winit window + softbuffer CPU painting, input, timer, trace.
                              Pure Rust: Windows, Linux, macOS.
android/    vetochki-android — Android NativeActivity entry point (cdylib) + APK metadata.
scenarios/  *.txt           — behavior spec, replayed on the model by the core tests.
```

---

## 0. Packaging and runtime (not ported)

| | |
|---|---|
| Behavior | `Vetochki.exe` is a WinRAR SFX: `Silent=1`, `TempMode`, `Overwrite=1`, `Setup=SP.exe` → silently unpacks `SP.exe` + `php5ts.dll` to %TEMP% and runs `SP.exe` **[C]**. `SP.exe` (UPX, Delphi) boots PHP 5.3.8, `bcompiler_read`s the SoulEngine loader, builds `Form1` from the embedded DFM and wires 5 event handlers **[C]**. |
| Decision | Dropped entirely. One native exe, assets embedded with `include_bytes!`. No temp files, no DLLs, no `data\` dir (`CFG data_dir` is never used by the handlers) **[C]**. |
| Engine features | Registry, process launch, shutdown, Chromium, `DisableTaskMng`, `myFunc1` … exist in SoulEngine but are never reached by this program **[C]** (all handlers decoded) → not ported. |

## 1. Main window

1. **Behavior.** A borderless window covering the whole primary monitor, caption `Hello Word`,
   colour `#E6E6E6` under the scene. No title bar, no resize. Closed with Alt+F4 → app exits.
2. **I/O.** In: display geometry, Alt+F4. Out: window.
3. **Constants.** DFM `BorderStyle=bsNone`, `Caption='Hello Word'`, `Color=15132390`;
   CFG formsInfo `windowState=wsMaximized`, `position=poScreenCenter`, `borderStyle=bsNone`
   (applied by SoulEngine `initForm` over the DFM) **[C]**. CFG `apptitle=SP` = Application.Title.
4. **APIs (original).** VCL TForm → `CreateWindowEx(WS_POPUP…)`, `ShowWindow(SW_MAXIMIZE)`.
   Manifest: Common-Controls 6, `asInvoker`, **not DPI aware** **[C]**.
5. **Uncertainty.** A maximized `WS_POPUP` window covers the taskbar **[L]**. Taskbar button text is
   `SP` in the original (classic Delphi: the hidden Application window owns the button) **[L]**; the
   reimplementation shows `Hello Word` — accepted deviation (one top-level window). Behavior on a
   secondary monitor is not reproduced beyond "primary display" **[?]**.
6. **Rust.** `winit` window: `with_decorations(false)`, `with_resizable(false)`,
   `Fullscreen::Borderless(None)` (the current monitor), title `Hello Word`. Alt+F4 →
   `event_loop.exit()`. The scene is composed at 96-DPI logical size and nearest-neighbour stretched
   to the physical surface when the display scale is not 100 % — the same thing Windows does to the
   DPI-unaware original. → `app/src/main.rs`.

## 2. Scene painting

1. **Behavior.** Background photo, leaf pile, branch; the branch moves. `label1` is never visible.
2. **I/O.** In: `Game::branch`. Out: pixels.
3. **Structs/constants** (`core/src/lib.rs`):

   | control | DFM rect | picture | flags | drawn at (form px) |
   |---|---|---|---|---|
   | `label1` TLabel | 624,16 80×24 | — | Transparent, font *TeamViewer11* bold −19 | covered by image3 |
   | `image3` TMImage | −8,−8 2048×1536 | 2048×1536 RGB | Mosaic, Proportional | −8,−8 2048×1536 |
   | `image2` TMImage | 72,120 328×200 | 800×348 RGBA | Center, Proportional | 72,149 328×142 |
   | `image1` TMImage | 584,136 208×144 | 640×485 RGBA | Center, Proportional | x+9,y 190×144 |
   | `edit1` TEdit | 112,40 104×21 | — | ReadOnly, TabOrder 4 | on top of everything |

   `dest_rect` reproduces VCL `TImage.DestRect` (Trunc, shrink-only, `div 2` centring) **[C]** for
   VCL's algorithm; that TMImage inherits it is **[L]** (`class TMImage extends TImage` in the PHP
   wrapper **[C]**, the Delphi side not inspected).
4. **APIs.** pngimage (TPNGObject) alpha drawing, GDI.
5. **Uncertainty.** Paint order = creation order for graphic controls, windowed controls above
   them — standard VCL **[C]**; hence `label1` (first) is hidden under the opaque background and is
   not drawn at all. Scaling filter differs (pngimage vs our box filter) → pixel diffs are expected,
   positions are not. `Mosaic` has no effect because picture size = control size **[C]**.
6. **Rust.** Assets are the original pictures re-encoded as **lossy WebP** (VP8 + lossless ALPH
   alpha), decoded once with `image-webp`, shrunk once to the DestRect sizes with a box filter, kept
   premultiplied. **Deviation:** on a window larger than 2040×1528 form pixels the original shows
   form colour right of or below the photo; here the photo is enlarged instead (bilinear, aspect
   kept, still anchored at −8,−8) just enough to cover the window, like CSS `background-size:
   cover` that never shrinks (`background_cover`). Smaller windows get it unscaled, as the original.
   Background colour + photo + leaf pile are baked into a static layer (the two
   pictures are decoded for that and dropped; rebuilt only on a size/DPI change). The frame is
   updated incrementally — at 100 % directly in softbuffer's window buffer (it keeps the previous
   frame), otherwise in a 96-DPI frame that is then stretched: only the old and new branch rectangles and the score box
   are restored from the layer and recomposed, only those are stretched into the window buffer and
   presented (`present_with_damage`), and nothing is redrawn while the visible state is unchanged
   (`app/src/main.rs`: `update_frame`, `copy_to_window`; a test checks the result equals a full
   repaint at 100/125/150/175 % scale). Lossy coding and
   the unknown pngimage resampling filter mean pixel values differ slightly from the original;
   geometry does not. The lossless original PNGs are not part of this repository.

   Format choice, measured with a throwaway probe build (same program and profile, only the image
   format differs; Linux x86-64 release):

   | format | images | code | total |
   |---|---|---|---|
   | PNG (`png`) | 2.34 MB | 2.28 MB | 4.62 MB |
   | AVIF (`avif-parse` + `rav1d`, no asm) | 0.47 MB | 3.36 MB | 3.83 MB |
   | **WebP (`image-webp`)** | 0.41 MB | 2.31 MB | **2.72 MB** |

   The WebP decoder costs about as much as the PNG one; the AV1 decoder ~1.1 MB more. AVIF crates
   built on `rav1d-safe` (`zenavif`) are AGPL-3.0 and were excluded.

## 3. Input and hit-testing

1. **Behavior.** Pressing any mouse button on the branch's rectangle (transparent corners included)
   starts dragging; releasing anywhere stops it. Presses on `edit1` never drag, even where the
   branch overlaps it. Everything else ignores the mouse.
2. **I/O.** In: button down/up + position. Out: `Game::mouse_down/up`.
3. **State.** `captured: bool` (VCL mouse capture of `image1`).
4. **APIs (original).** `WM_*BUTTONDOWN/UP` → `TControl.MouseDown/Up`; capture via `SetCapture`.
5. **Uncertainty.** Handlers ignore `$button` **[C]**. VCL sets mouse capture only for the *left*
   button **[L]** (VCL source knowledge, not checked in SP.exe), so a right-button release after the
   branch snapped home might not reach `onmouseup` — but the timer is already off at that point, so
   the observable result is the same either way **[C]**. Double-click = second press **[L]**.
6. **Rust.** `core::hit_test` (edit first, then image1 rect, half-open like `PtInRect`) on winit
   `MouseInput` presses of any button; the release is routed back while `captured` is set.

## 4. Timer and game logic (the state machine)

1. **Behavior.** While dragging, every 10 ms the branch is centred on the cursor. If its top-left
   enters `x < 150 && y < 150` (over the leaf pile) it snaps home, dragging stops and the score
   goes up by one. The very first press of a session scores immediately (quirk below).
2. **I/O.** In: timer ticks, cursor *screen* position. Out: `branch`, `label`, `score`, `timer`.
3. **State machine** (`Game`, all fields are the original properties):

   ```
   Idle(timer=0) --press on branch--> Dragging(timer=1)
   Dragging --release--> Idle
   Dragging --tick--> branch := cursor-(104,72)
                      if branch.x<150 && branch.y<150: label := "2"
                      if label >= "2" (PHP):           branch := (584,136); label := "0";
                                                       timer := 0; score := score + 1 (PHP)
   ```
   Initial: `branch=(584,136)`, `label="Счет"`, `score="Счет"`, `timer=0`. **[C]** (L27–L47).
4. **APIs.** `cursor_pos_x/y()` = host function `libScreenFunctions3/4Execute` → `TMouse.GetCursorPos`
   → `USER32!GetCursorPos` (screen coordinates) **[C]** (disassembled). Timer = SoulEngine
   `TFuncTimer` → `TTimerEx` → VCL `TTimer`, PARAMS `interval='10'`, `repeat=true`,
   `enable=false` **[C]**.
5. **Uncertainty / quirks.**
   * **Free first point [C]:** the second `if` is not an `elseif` (`JMP @48` falls through), and
     PHP's `"Счет" >= "2"` is a byte-wise comparison (non-numeric operand) that is true for the
     caption in cp1251, UTF-8 or `????` — so tick 1 resets and scores.
   * `label1` is a hidden flag; `"2"` never survives a tick.
   * The real `TTimer` period is the system timer granularity (~15.6 ms by default), not 10 ms
     **[L]**; the reimplementation uses a 10 ms timer. Only matters for very fast swipes across
     the zone.
   * Re-enabling the timer restarts its phase (`TTimer.SetEnabled` → `KillTimer`/`SetTimer`)
     **[L]** (VCL source knowledge); modelled by a generation counter.
   * Score past 2³¹−1 would switch PHP to float formatting **[C]**; unreachable in practice.
6. **Rust.** `core::Game::{mouse_down, mouse_up, tick}`; PHP rules in `php_ge` / `php_add`. Timer =
   `ControlFlow::WaitUntil(next_tick)`; enabling it schedules `now + 10 ms`, and a late wake-up gives
   one tick rather than a burst, like `WM_TIMER`. Cursor = `window.inner_position()` + last
   `CursorMoved` position (screen coordinates, scaled to 96 DPI).

## 5. `edit1` (score box)

1. **Behavior.** White read-only box, Tahoma 8 pt, shows `Счет`, then `1`, `2`, …
2. **I/O.** In: `Game::score`. Out: text.
3. **Constants.** DFM `Text='Счет'`, `TextHint='Счет'` (never shown: text never empty), `ReadOnly`.
4. **APIs.** comctl32 v6 Edit (themed), `WM_SETTEXT`.
5. **Uncertainty.** As the only tab stop it should get focus at startup and `AutoSelect` selects
   all text until the first `Text` assignment **[L]** — reproduced as a highlight; no blinking caret
   is drawn. Theme metrics (border colour, inner margin) are Windows-10 approximations **[L]**.
6. **Rust.** Drawn box (1 px `#7A7A7A` border, white, 3 px text inset) with `fontdue` text at an
   11 px em. Font: the system Tahoma when present (Windows, macOS), else an embedded 2.9 KB subset
   of DejaVu Sans with exactly the glyphs the box can show (`Счет`, `0`–`9`; DejaVu licence in
   `app/assets/`). Only these glyphs are rasterised, once, and the font is then dropped: fontdue
   keeps every glyph of a font parsed (12.7 MB for the system Tahoma). `edit_selected` highlight
   while the window is focused; no caret.

## 6. Dead or inert code (deliberately not ported)

| item | why |
|---|---|
| `Form1.onshow: doubleBuffered = 1` | anti-flicker only; softbuffer presents whole frames **[C]** |
| `eval(enc_getValue('__incCode'))` in every handler | SoulEngine's shared prelude for `eval`ed code; nothing app-specific **[C]** |
| `global $sx, $sy, $fx, $fy` | declared, never used **[C]** |
| `myFunc1` (`label1->caption += '1'`) | registered as PHP function `myFunc1()`, never called **[C]** |
| `label1` font *TeamViewer11* | label is never visible **[C]** |

## 7. Android

The same `vetochki` library runs as a NativeActivity: `android/src/lib.rs` defines `android_main`
and passes winit's Android event loop to `vetochki::run`. On other targets that crate is empty.
`app/src/main.rs` is the desktop entry point.

| | desktop | Android |
|---|---|---|
| scale | display scale (DPI-unaware stretch) | largest that fits an 800×600 form on the screen; the rest shows background |
| input | mouse, any button | first finger = mouse (`WindowEvent::Touch`); other fingers ignored |
| exit | Alt+F4 | Back key (winit marks every key handled, so it is mapped explicitly) |
| full screen | `Fullscreen::Borderless` | theme `NoActionBar.Fullscreen`: status bar hidden, navigation bar stays |
| window | created once | dropped in `suspended`, recreated in `resumed`; a drag in progress ends on suspend |
| redraw | incremental, damage rectangles | whole frame every time: softbuffer's Android backend reports buffer age 0 and ignores damage |

The activity is landscape only (`sensorLandscape`: either side up). A rotation is a resize
(`configChanges` in the manifest), so the game state survives it. The font
falls back to the embedded DejaVu subset (no Tahoma on Android). Only `aarch64-linux-android` is
built; minimum Android 7 (API 24), target API 35. `android/debug.keystore` (password `android`) is
a throwaway key for side-loading.

---

## Testing

* `cargo test -p vetochki-core` (any OS): unit tests for layout/PHP rules/state machine, plus every
  `scenarios/*.txt` replayed on the deterministic simulator with its `check` expectations.
* `cargo test -p vetochki`: the WebP assets decode as expected, and incremental redraw leaves
  exactly the frame a full repaint gives (100/125/150/175 % scale).

The scenario expectations encode the reconstruction's claims (free first point, strict `< 150`,
any button, edit never drags …).

**Comparison with the original.** Earlier revisions carried `compare/` (`vetochki-compare`), a
Windows harness that replayed the same scenario files with real input (`SetCursorPos`,
`SendInput`) on the original `Vetochki.exe` and on this program, read the original's score from
its Edit control (`WM_GETTEXT`) and the reimplementation's full state from `VETOCHKI_TRACE`,
captured screenshots at every `check`, and diffed two runs (shared observables + 8×8-block
screenshot difference). It was removed because the original executable is not part of this
repository. It was not run against the original, so the scenario expectations are confirmed on
the model, not on `Vetochki.exe`; the running program was checked by hand on Windows.

## Building

Pure Rust, no C/C++ code in the dependency tree: no Visual Studio, Windows SDK or MinGW needed.

```powershell
# Windows: the GNU toolchain links with its bundled linker (the MSVC one would need link.exe)
rustup toolchain install stable-x86_64-pc-windows-gnu
cd <project>
cargo +stable-x86_64-pc-windows-gnu test  -p vetochki-core
cargo +stable-x86_64-pc-windows-gnu build --release -p vetochki
.\target\release\vetochki.exe
```
Linux/macOS: `cargo build --release -p vetochki`.
Android: `cargo apk2 build -p vetochki-android --release`, setup in the [README](../README.md#android).

**Releases (CI).** `.github/workflows/release.yml` runs manually only (`workflow_dispatch`). It
builds on four native runners in parallel — `ubuntu-26.04`, `ubuntu-26.04-arm`, `windows-2025`,
`windows-11-arm` (MSVC there: Visual Studio is preinstalled and Rust has no `aarch64` GNU target for
Windows) — running `cargo test` first. With a `tag` input the four binaries are published as a
GitHub release (the tag is created on the built commit if missing); without one it only builds.
Dependencies are cached with `Swatinem/rust-cache`; runs on `master` save the cache.

**Local paths in the binary.** Panic sites embed their source location, and cargo passes registry
crates to rustc by absolute path, so even a stripped exe contains `<CARGO_HOME>\registry\src\…`
(user name included). `trim-paths` is still unstable in cargo 1.99; on stable remap the prefix
when building (not in the repo config — that would put the user name into the repo instead):

```powershell
$env:RUSTFLAGS = "--remap-path-prefix=$env:USERPROFILE\.cargo\registry\src=registry"
```
or once per machine in `%USERPROFILE%\.cargo\config.toml`: `[build] rustflags =
["--remap-path-prefix=C:\\Users\\<user>\\.cargo\\registry\\src=registry"]`. Workspace crates are
already passed by relative path, and std ships remapped to `/rustc/<hash>/…`.

`[profile.release]` trades speed for size (`strip`, fat LTO, `opt-level = "z"`, `panic = "abort"`);
winit is built without Wayland client-side decorations (the window is borderless).
Linux release: 2.07 MB, of which 0.41 MB are the three WebP pictures (the Linux build also
carries the X11 + Wayland backends). Windows release: 1.08 MB, ~30 MB RAM at 1920×1080 (the
original: 6.5 MB on disk, ~35 MB RAM).

History: a first shell used gpui; its dependency tree (image codecs incl. an AV1 encoder, SVG
renderer, HTTP/TLS stack, DirectX shaders compiled by `fxc`) made a 36 MB exe that needed MinGW
and an `fxc` stand-in to build without Visual Studio, so it was replaced.

**Verified:** `cargo test` (core: 7 unit tests + 6 scenarios; app: asset decoding + incremental
redraw equivalence); `cargo clippy --workspace` clean for `x86_64-pc-windows-msvc` and Linux; Linux
and Windows release builds; the Windows `.exe` run by hand.
