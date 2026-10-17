# vetochki

An attempt to reverse engineer and rewrite of my friend’s old application using AI agents.

![vetochki screenshot](./screenshot.jpg)

## How to build

Windows, pure Rust — no Visual Studio, Windows SDK or MinGW needed.

1. Install Rust with [rustup](https://rustup.rs), then switch to the GNU toolchain (it links with
   its bundled linker; the default MSVC one needs Visual Studio's `link.exe`):

   ```powershell
   rustup toolchain install stable-x86_64-pc-windows-gnu
   rustup default stable-x86_64-pc-windows-gnu
   ```

2. Build and run:

   ```powershell
   git clone https://github.com/e965/vetochki.git
   cd vetochki
   cargo test
   cargo build --release -p vetochki
   .\target\release\vetochki.exe
   ```

   The window is borderless full-screen; close it with Alt+F4.

Optional: keep your local paths (`C:\Users\<you>\.cargo\…`) out of the exe by setting this before
building:

```powershell
$env:RUSTFLAGS = "--remap-path-prefix=$env:USERPROFILE\.cargo\registry\src=registry"
```

### Android

Built on Linux (WSL works) into a side-loadable APK signed with a debug key from the repo.

```bash
# Android SDK command-line tools unpacked to ~/Android/Sdk/cmdline-tools/latest, then:
export ANDROID_HOME=~/Android/Sdk
export ANDROID_NDK_ROOT=$ANDROID_HOME/ndk/29.0.14206865
$ANDROID_HOME/cmdline-tools/latest/bin/sdkmanager --licenses
# NDK r29 supports API levels up to 35, so platforms;android-35 is required
$ANDROID_HOME/cmdline-tools/latest/bin/sdkmanager "platform-tools" "build-tools;37.0.0" \
    "platforms;android-35" "ndk;29.0.14206865"
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android i686-linux-android
cargo install cargo-apk2 --locked

cargo apk2 build -p vetochki-android --release   # target/release/apk/vetochki.apk
```

Copy the APK to the phone and open it, or install it with `adb install`. The Back key closes the app.

More details (Linux/macOS, size and memory notes) in [docs/Design.md](docs/Design.md).

### Web

The `vetochki-web` crate runs the same game and CPU renderer in a browser canvas. Mouse and
touch work; the scene scales to fit the viewport, including after resizing or rotating a phone.

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk --locked
cd web
trunk serve --open
```

For a static hosting build, run `trunk build --release` from `web/` and publish `web/dist/`.
Resource URLs default to `./` via `web/Trunk.toml`. Serve the files over HTTP(S). Images and the
fallback font are embedded in WASM; no backend is required. The game fills the page without
requesting browser fullscreen.

The `release` workflow also builds the web version. With a non-empty `tag`, it deploys to GitHub
Pages after publishing the release. In the repository's **Settings → Pages → Build and deployment**,
set **Source** to **GitHub Actions** before the first deployment. Resource URLs are relative, so
the build works under the repository's Pages path as well as a custom domain.

## Verify a release download

New workflow builds include build provenance attestations for the Windows executables, Linux
archives and Android APK. Verify a downloaded file with the [GitHub CLI](https://cli.github.com/):

```bash
gh attestation verify vetochki-android.apk --repo e965/vetochki
```

Replace the filename with the executable or archive you downloaded.

## Thanks

* [Ghidra](https://github.com/NationalSecurityAgency/ghidra)
* [ghidra-rpc](https://github.com/cellebrite-labs/ghidra-rpc)
* Claude Code

## License

Code - [VIBECODE PUBLIC LICENSE](./LICENSE)

Images - idk