# vetochki

An attempt to reverse engineer and rewrite of my friend’s old application using AI agents.

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

More details (Linux/macOS, size and memory notes) in [docs/Design.md](docs/Design.md).

## Thanks

* [Ghidra](https://github.com/NationalSecurityAgency/ghidra)
* [ghidra-rpc](https://github.com/cellebrite-labs/ghidra-rpc)
* Claude Code

## License

Code - [VIBECODE PUBLIC LICENSE](./LICENSE)

Images - idk