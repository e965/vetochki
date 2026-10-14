# Static Analysis — Vetochki.exe

Static-only reverse engineering. **The sample was never executed.** The embedded RAR was
*decompressed* (data unpacking, not execution) and the inner files analyzed statically; no
extracted binary was run.

Evidence tags: **[C]** confirmed from code/structure · **[L]** likely inference · **[?]** unknown.

Tools: ghidra-rpc (PE/code), Python (RAR4 + SoulEngine block parsing, zlib, bcompiler opcode
+ DFM parsing), 7-Zip 23.01 (RAR decompression), UNRAR 7.12 (RAR comment / SFX script).
Working files are kept outside this repository; see §10.

---

## TL;DR

`Vetochki.exe` is a **four-layer nesting doll**:

1. **WinRAR SFX stub** (Borland C++, 32-bit) — a self-extracting archive. **[C]**
2. Appended **RAR4 archive** holding two files: `php5ts.dll` (stock PHP 5.3.8 runtime) and
   `SP.exe` (the real application). **[C]**
3. `SP.exe` — a **UPX-packed Delphi app** built with **"SoulEngine" (Dim-S Software, 2009)**,
   a Russian PHP-scriptable RAD runtime that embeds `php5ts.dll` + `bcompiler`. The app's
   PHP is stored in named, zlib-compressed, partly bytecode-compiled blocks inside the EXE. **[C]**
4. The **actual program**: one borderless full-screen form with 3 images, a label, an edit
   box and a timer; the timer tracks the cursor position over an image. A small interactive
   graphics app. **[C]** structure / **[L]** purpose.

**No networking, no real cryptography, no licensing/DRM, no anti-debug** at the application
level. The *engine* exposes powerful capabilities (registry, process launch,
shutdown/logoff, disable-Task-Manager, embedded Chromium) but **this app instantiates none
of the network/browser controls and calls none of those APIs** — now **[C]**, because all
5 event handlers were decompiled from bytecode (§4.5) and the `__incCode`/`enc_*` layer was
shown to be inert plumbing, not encryption (§4.4). The outer SFX silently extracts to a temp
folder and auto-runs `SP.exe` (`Setup=SP.exe`, `Silent=1`, `TempMode` — §2). Everything in
the sample is resolved by static analysis.

---

## 1. Layer map

```
Vetochki.exe  (6,826,510 bytes)
├─ 0x00000000  PE stub — WinRAR SFX module (Borland C++, x86)         [layer 1]
└─ 0x00038000  RAR4 archive (overlay, ~6.55 MB)                        [layer 2]
   ├─ php5ts.dll   6,831,616 B  — PHP 5.3.8 Thread-Safe runtime (stock)
   └─ SP.exe       4,141,245 B  — UPX-packed Delphi app                [layer 3]
      └─ overlay: SoulEngine named blocks (SO!#$PHPSOULENGINE\...)     [layer 4]
         ├─ INC.PHP      bootstrap (plaintext PHP)
         ├─ LOADER       bcompiler bytecode (gz)  — DS_Loader + enc_* 
         ├─ SOULENGINE   231 KB PHP source — the RAD framework/VCL bridge
         ├─ X_MODULES    47 KB PHP source — app framework classes
         ├─ F\XFORMS     5.5 MB — serialized form (controls + image data)
         ├─ X_CONFIG     base64(serialize) — app + form config
         ├─ _EVENTS      event→handler wiring
         ├─ _EXEVFILE    bcompiler bytecode — the app's event handlers (obfuscated)
         └─ PHPINI, *.HASH, SIGN, WARNING.CHECK, FOR_ANTIVIRUS_START, ...
```

---

## 2. Layer 1 — WinRAR SFX stub (the outer PE)

| Field | Value | Evidence |
|-------|-------|----------|
| Format / arch | PE32, x86 32-bit LE, base `0x400000`, fixed base (no `.reloc`) | **[C]** |
| Compiler | Borland C++ (`borlandcpp`) | **[C]** Ghidra id |
| Entry | `entry` @ `0x401000` (sole export) | **[C]** |
| Sections | Headers / .text(80K) / .data(32K) / .idata(8K) / .rsrc(137K) | **[C]** |
| Identity | **WinRAR SFX module** | **[C]** strings |

SFX fingerprints **[C]**: `RarSFX`, `Software\WinRAR SFX`, `sfxcmd`, `sfxname`,
`__tmp_rar_sfx_access_check_%u`, `RarHtmlClassName`, `__rar_`, dialog ids
`STARTDLG`/`LICENSEDLG`/`GETPASSWORD1`/`ASKNEXTVOL`/`REPLACEFILEDLG`/`RENAMEDLG`, the
ShellExecute template `-el -s2 "-d%s" "-p%s" "-sp%s"`, elevation verb `runas`, and
`Shell.Explorer`/`about:blank` (HTML dialog). Russian RAR extraction strings
(`Извлечение %s`, `Ошибка CRC…`, `Неверный пароль…`, `Архив повреждён…`).

Imports that belong to the stub (149 total) **[C]**: registry (`Reg*`), filesystem
(`CreateFile*`/`Find*File*`/`Move*`/`Delete*`/`SetFileTime`/`SetFilePointer`), shell
(`SHFileOperationA`, `SHBrowseForFolderA`, `ShellExecuteExA`), privilege
(`OpenProcessToken`+`LookupPrivilegeValueA`+`AdjustTokenPrivileges`, privileges
`SeRestorePrivilege`/`SeSecurityPrivilege`), `SetFileSecurity*`, file dialogs, OLE. **No
network, no Crypt\*** in the stub's import table. Mapped code functions (renamed in Ghidra,
§9) **[C]**: `sfx_shell_execute` (ShellExecuteExA×2 — launches `SP.exe`/`runas`),
`sfx_enable_privilege` (AdjustTokenPrivileges), `sfx_reg_read`/`sfx_reg_write`.

**SFX run-script — recovered** (RAR archive comment, via UNRAR 7.12) **[C]**:
```
;Расположенный ниже комментарий содержит команды SFX-сценария   (“the comment below holds SFX script commands”)
Setup=SP.exe
TempMode
Silent=1
Overwrite=1
Update=U
```
Behavior **[C]**: `Silent=1` → **no extraction UI** (none of the `STARTDLG`/`LICENSEDLG`
dialogs shown); `TempMode` → extract to a **temporary folder**; `Overwrite=1` → overwrite
without prompting; `Setup=SP.exe` → **auto-launch `SP.exe` after extraction**; `Update=U`.
⇒ `Vetochki.exe` is a **silent run-from-temp launcher**: it unpacks `SP.exe`+`php5ts.dll`
to a temp dir and runs `SP.exe` with no user interaction. (`SP.exe` is launched via the
stub's `sfx_shell_execute`; `runas` is only used if elevation is requested — not here.)
This is the normal way to ship a PHP+runtime app as one self-contained EXE. **Nothing in
this binary is now unresolved by static analysis.**

---

## 3. Layer 2 — the archive contents

RAR4 overlay at file offset `0x38000`, single volume, 2 files, **not encrypted** (extracted
with no password). **[C]**

| File | Size | Type | Notes |
|------|------|------|-------|
| `php5ts.dll` | 6.83 MB | PE32 DLL | **PHP 5.3.8** TS/VC9 (`X-Powered-By: PHP/5.3.8`, Zend Engine v2.3.0, API20090626). **Stock runtime** — not analyzed further. **[C]** |
| `SP.exe` | 4.14 MB | PE32 GUI, i386, **UPX-compressed**, 3 sections | The application. Barely RAR-compressible because already UPX-packed. Build date 2016-08-01. **[C]** |

sha256: `SP.exe` `414d26e0…2d07a4`, `php5ts.dll` `9b2c8b8c…63baea4`.

---

## 4. Layer 3/4 — inside SP.exe

### 4.1 Packaging
- **UPX-packed** (`NRV` stub). UPX only decompresses on load; the SoulEngine blocks sit in
  the UPX overlay and are readable without unpacking. **[C]**
- Delphi host. Embeds PHP via **PHP4Delphi-style classes** (`TSynPHP`, `TPHPLib`) and runs
  compiled PHP through **bcompiler v0.27**. **[C]** strings.
- Engine = **SoulEngine, "Dim-S Software (c) 2009"** (hence `DS_Loader`, `DSApi`). **[C]**
  in-source banners.

### 4.2 Bootstrap chain **[C]**
`INC.PHP` (plaintext):
```php
$loader = exemod_extractstr('$PHPSOULENGINE\\loader');   // native host fn reads embedded block
$loader = gzuncompress($loader);                          // -> bcompiler bytecode
bcompiler_read( php://memory(loader) );                   // load compiled classes
$LOADER = new DS_Loader;                                  // engine entry object
```
So the EXE provides a native `exemod_extractstr()` that returns the named `$PHPSOULENGINE\…`
blocks; PHP inflates and `bcompiler_read`s them. Integrity is checked via per-block MD5
(`*.HASH`) and `SIGN`/`SIGN.CHECK`/`WARNING.CHECK` blocks. **[C]** blocks exist; **[L]** that
they are verified at load.

### 4.3 Recovered PHP (decompressed, §9 for paths)
| Block | Form | What it is |
|-------|------|-----------|
| `SOULENGINE` | PHP source, 231 KB | The RAD framework: ~700 functions + ~180 VCL wrapper classes (`TForm`,`TEdit`,`TImage`,`TRegistry`,`TChromium`,`TThread`,`TFileStream`,`TSynEdit`…). **[C]** |
| `X_MODULES` | PHP source, 47 KB | App-layer classes: `app`, `DSApi`, `eventEngine`, `animate`, `Geometry`, `Data`, `ini`, `TFunction`, `TFuncTimer` + the full `On*` event set (incl. Chromium events). **[C]** |
| `F\XFORMS` | serialized, 5.5 MB | Form1 definition + embedded image (`TMImage`) data. **[C]** |
| `X_CONFIG` | base64(serialize) | App/form config (below). **[C]** |
| `LOADER` / `_EXEVFILE` | bcompiler bytecode | Loader + the app's per-handler code (obfuscated). **[C]** |

### 4.4 Code-protection / `__incCode` — resolved **[C]**
Initially this looked like an encrypted per-handler store (`enc_getValue`/`__incCode`). It is
**not**. From `X_MODULES`:
```php
static function runCode($code,$self){ return eval( enc_getValue('__incCode') . $code ); }
// handler dispatch:  if (defined('DEBUG_OWNER_WINDOW')) include callFileName(...);  else eval(getEvent(...));
```
`__incCode` is merely a **PHP prelude string prepended to every `eval`** (common helper code);
`enc_*` is a trivial `urlencode(serialize())` key-value table, **not a cipher**. In **release**
mode handlers run as the compiled `_EXEVFILE` bytecode; the `include` of the dev file
`C:\Users\…\AppData\Local\Temp\1295.php` happens **only if `DEBUG_OWNER_WINDOW` is defined**
(design/debug). This build has `debug.enabled=false`. ⇒ **No hidden/encrypted application
logic. The leaked temp path is a build-time debug artifact, never used at runtime.** **[C]**

### 4.5 Application logic — exact handler source **[C]**
*Revised.* The first pass read only the order of string constants and misread several handlers.
A proper disassembler for bcompiler v0.27 / Zend 2.3, written from the bcompiler 1.0.2 and PHP
5.3.8 sources (`analysis/scripts/bcdecode.py`), consumes `_EXEVFILE` byte-exactly
(0x2394 of 0x2394) and gives the source with jump targets. Full listing in
`analysis/handlers_reconstructed.php`:

```php
Form1.onshow:          c('Form1')->doubleBuffered = 1;
image1.onmousedown:    c('timer1')->enable = 1;                 // any button
image1.onmouseup:      c('timer1')->enable = 0;
myFunc1.onexecute:     c('label1')->caption += '1';             // never called
timer1.ontimer:        global $sx, $sy, $fx, $fy;               // unused
                       c('image1')->x = cursor_pos_x() - 104;   // centre the branch on the cursor
                       c('image1')->y = cursor_pos_y() - 72;
                       if (c('image1')->x < 150 && c('image1')->y < 150) c('label1')->caption = '2';
                       if (c('label1')->caption >= '2') {       // separate if, JMP @48 falls through
                           c('image1')->x = 584;  c('image1')->y = 136;   // back home
                           c('label1')->caption = '0';
                           c('timer1')->enable = 0;
                           c('edit1')->text += '1';             // score
                       }
```

Component params (igbinary in DFM `HelpKeyword`, `analysis/scripts/dfm_params.py`): `timer1` =
TFuncTimer **interval 10 ms, repeat, initially disabled**; `myFunc1` = TFunction registered as PHP
function `myFunc1()` that nothing calls. `cursor_pos_x/y()` are host functions
(`libScreenFunctions3/4Execute` in the Delphi data module) that call `TMouse.GetCursorPos` →
`USER32!GetCursorPos` — **screen** coordinates (disassembled in the UPX-unpacked SP.exe).

⇒ **A small game: drag the branch (image1) onto the leaf pile; when its top-left corner gets
into `x < 150, y < 150` it snaps back and the score in `edit1` goes up.** `label1` is a hidden
flag: it is painted first and the opaque background covers it. Quirk: `"Счет" >= "2"` is true
under PHP's byte-wise string comparison, so the first press of a session scores a free point.
No file, registry, network, process or crypto calls in any handler. **[C]**

---

## 5. Focus-area findings (application level)

| Area | Verdict | Evidence |
|------|---------|----------|
| **Initialization** | `Vetochki.exe` SFX **silently** (`Silent=1`,`TempMode`) extracts `SP.exe`+`php5ts.dll` to a temp folder and **auto-runs `SP.exe`** (`Setup=SP.exe`). `SP.exe` then: UPX self-unpacks → loads `php5ts.dll` → `exemod_extractstr`→`gzuncompress`→`bcompiler_read`→`new DS_Loader` → builds Form1 from `F\XFORMS`/`X_CONFIG` → shows the full-screen form. | **[C]** |
| **Config loading** | In-EXE blocks: `X_CONFIG` (base64+serialize), `PHPINI` (embedded php.ini), `F\XFORMS`. `data_dir="data"` ⇒ a `data\` folder beside the EXE at runtime. | **[C]** blocks / **[L]** the `data\` runtime dir |
| **Filesystem** | SFX writes payload to **%TEMP%** (TempMode). Engine-wide: `TFileStream`, `loadFromFile`/`saveToFile`, `deleteDir`/`rmdir_recursive`, `fileLock`/`dirLock`. App *handlers* make **no** FS calls (decompiled — §4.5). | **[C]** |
| **Registry** | Engine `TRegistry` + `DisableTaskMng()` (writes `HKCU\…\Policies\System\DisableTaskMgr=1`). SFX stub reads `HKLM\…\CurrentVersion\ProgramFilesDir` and `Software\WinRAR SFX`. App handlers call **no** registry API (decompiled). | **[C]** capability; **[C]** app makes no calls |
| **Networking** | **None at app level.** No ws2_32/wininet/winhttp in either PE import table; app form has no `TChromium`/`TWebBrowser`; no `loadUrl`/`fsockopen`/`curl` in recovered app code. Engine *can* browse (CEF) but app doesn't. | **[C]** absence |
| **Crypto** | **No real crypto.** No CryptoAPI imports. Only gzip (`gzuncompress`), base64, PHP `serialize`, MD5 integrity hashes, and the `enc_*` obfuscation store. RAR payload itself is unencrypted. | **[C]** |
| **Auth / licensing** | **None found.** "serial/license/activat" string hits were false positives (`serialize`, a `@license new-bsd` docblock, `AW_ACTIVATE`/`onActivate`). | **[C]** |
| **Process creation** | SFX stub launches **`SP.exe`** post-extract via `sfx_shell_execute`/ShellExecuteExA (`Setup=SP.exe` — confirmed). Engine also exposes `execute`, `localApplication`, `winShutdown/Restart/Logoff/Sleep/Exit`, but the app handlers invoke **none** of them (decompiled). | **[C]** |
| **Anti-debug / anti-analysis** | No import-level anti-debug in either PE (no IsDebuggerPresent/OutputDebugString). Obstacles are *packing/obfuscation* (SFX → UPX → bcompiler → enc store) and SFX `SIGN`/`WARNING.CHECK`/`FOR_ANTIVIRUS_START` integrity/AV-evasion blocks. `DisableTaskMng` exists but isn't shown to be called. | **[C]** absence of classic anti-debug / **[L]** layering as anti-analysis |

---

## 6. Capability vs. use — important caveat

SoulEngine is a **general-purpose RAD runtime**; its 231 KB engine naturally contains
process-launch, shutdown, registry and Chromium APIs regardless of this app. That these
capabilities belong to the engine (not to this program) is now **[C]**, because all 5 event
handlers were decompiled from the `_EXEVFILE` bytecode (§4.5) and the `__incCode`/`enc_*`
"obfuscation" was shown to be an inert eval-prelude/debug hook (§4.4): the handlers do nothing
but set form/image/label properties and toggle the timer. Combined with the **absence** of any
network/browser control in the form and of any network/crypto imports in both PEs ⇒ **benign
full-screen nature-scene / casual-demo app.** The outer SFX run-script is now recovered too
(§2: silent temp-extract + auto-run `SP.exe`). **Every layer is resolved by static analysis;
no residual unknowns.** Overall: **[C] benign.**

---

## 7. Artifacts / IOCs   **[C]**

- Engine: **SoulEngine**, "Dim-S Software (c) 2009"; app title `SP`; packer markers
  `$PHPSOULENGINE\…`, `exemod_extractstr`, `bcompiler v0.27`.
- Runtime: PHP 5.3.8 TS (`php5ts.dll`), UPX, WinRAR SFX.
- Leaked dev path: `C:\Users\…\AppData\Local\Temp\1295.php` (bcompiler metadata).
- Registry touched by engine API: `HKCU\Software\Microsoft\Windows\CurrentVersion\Policies\System\DisableTaskMgr`.
- Build dates: inner EXE/DLL 2016-08-01.
- Hashes: see §3.

---

## 8. Forms & assets (DFM) — recovered   **[C]**
`F\XFORMS` = PHP-serialized wrapper around a 5.5 MB **Delphi DFM** `object fmEdit: TForm`
(`Caption='Hello Word'`, `BorderStyle=bsNone`, 1920×1080, `Visible=False`). Components:

| Name | Class | Geometry / notable props |
|------|-------|--------------------------|
| `fmEdit` | TForm | borderless full-screen, color 15132390 |
| `image3` | TMImage | 2048×1536 @ (−8,−8), `Mosaic=True` — **background** |
| `image2` | TMImage | 328×200 @ (72,120), Center — leaf-pile sprite |
| `image1` | TMImage | 208×144 @ (584,136), Center — branch sprite |
| `label1` | TLabel | Caption `"Счет"` (Score), font *TeamViewer11* bold |
| `edit1` | TEdit | ReadOnly, Text `"Счет"` |
| `myFunc1`,`timer1` | __TNoVisual | logic objects |

`HelpKeyword` fields base64-decode to CP1251 Russian labels (`Изображение1/2`, `Текст1`).
Three **PNG images** carved from `Picture.Data` (`TPNGObject`): a blurred forest/park clearing
with a sapling (background), a pile of autumn leaves, and a green leafy **branch** ("веточка").
⇒ a nature-themed scene; with `"Счет"` + drag/coordinate handlers it is a small casual/demo
app (the `'Hello Word'` caption suggests a SoulEngine "hello-world"-style sample). **[L]** purpose.
The three PNGs were saved alongside the analysis (`image3_background.png`, `image2_leaves.png`,
`image1_branch.png`; not in this repository — the app ships them re-encoded as WebP in
`app/assets/`).

## 9. Renames applied in Ghidra   **[C]**
Six evidence-backed renames applied to the SFX stub (persisted in the project):
`sfx_shell_execute` (0x4026f1, ShellExecuteExA×2), `sfx_enable_privilege` (0x40a5d4,
AdjustTokenPrivileges), `sfx_reg_read` (0x402b84), `sfx_reg_write` (0x402b04),
`sfx_set_file_security` (0x40a684), `sfx_file_open_dialog` (0x40596c).

## 10. Recovered-files map (analysis working directory, not in this repository)
The first pass worked in a session scratchpad that no longer exists; everything was re-extracted
reproducibly into a separate `analysis/` directory:
- `analysis/extracted/` (untrusted data) → `SP.exe`, `php5ts.dll`, `SP.unpacked.exe` (UPX -d),
  `blocks/` = every `$PHPSOULENGINE` block inflated (`SOULENGINE`, `X_MODULES`, `_EXEVFILE`,
  `F__XFORMS`, `X_CONFIG`, …) + `Form1.dfm`.
- `analysis/scripts/` → `carve_soul.py` (blocks), `bcdecode.py` (bcompiler v0.27 disassembler),
  `dfm_params.py` (DFM + igbinary component params).
- `analysis/handlers_reconstructed.php` → exact handler source.
- `analysis/tools/` → UNRAR 7.12, UPX 4.2.4; `tools/src/` → bcompiler 1.0.2 and PHP 5.3.8 headers
  used as the format reference.
- `images/` (next to it) → the 3 PNGs.

## 11. Open questions — all resolved
1. ~~SFX script~~ — **RESOLVED** (§2, via UNRAR 7.12): `Setup=SP.exe` + `Silent=1` + `TempMode`
   ⇒ silent temp-extract and auto-run of `SP.exe`.
2. ~~Per-handler app logic~~ — **RESOLVED** (§4.4–4.5): all 5 handlers decompiled from bytecode;
   `__incCode`/`enc_*` is inert plumbing, not encryption.
3. ~~`F\XFORMS`~~ — **RESOLVED** (§8): component tree + 3 embedded PNGs extracted and viewed.

Nothing in the sample remains unresolved by static analysis.

## 12. Reimplementation (this repository)
Native Rust version of the program without SFX, PHP or SoulEngine; per-subsystem analysis
(behavior, I/O, state machine, APIs, uncertainty) in [Design.md](Design.md).
- `core/` — GUI-free model: DFM constants, VCL `TImage.DestRect`, the PHP comparison/addition rules
  the game depends on, the handler state machine, and a scenario simulator. `cargo test` runs unit
  tests plus every `scenarios/*.txt` with its expectations.
- `app/` — `winit` + `softbuffer` shell: borderless full-screen window, CPU compositing of the three
  pictures (re-encoded as lossy WebP, 0.41 MB), the score box (system Tahoma or an embedded 2.9 KB
  DejaVu subset), mouse capture, 10 ms timer. Pure Rust, builds for Windows/Linux/macOS; Windows
  release 1.08 MB, ~30 MB RAM. PNG / AVIF / WebP were measured side by side ([Design.md](Design.md) §2).
  A first gpui-based shell produced a 36 MB exe and needed MinGW + an `fxc` stand-in, so it was
  replaced.
- A Windows harness (`compare/`) for replaying the scenarios on the original `Vetochki.exe` existed
  in earlier revisions and was removed with the original executable; see [Design.md](Design.md),
  "Testing".
