# Runtime Capture on Linux with Proton

The runtime sniffer must be loaded into the official 32-bit `Gw.exe` process.
On Linux, launch the 32-bit injector through the same Proton prefix as Guild
Wars. Running the injector as a native Linux process, through another Wine
prefix, or before `Gw.exe` exists cannot work.

## 1. Build the 32-bit Windows artifacts

Install the Rust target and [`cargo-xwin`](https://github.com/rust-cross/cargo-xwin)
once:

```bash
rustup target add i686-pc-windows-msvc
cargo install --locked cargo-xwin
```

From the repository root, build both the injector and DLL:

```bash
bun run build:capture
```

`build:capture` sets `XWIN_ARCH=x86` so `cargo-xwin` downloads the 32-bit MSVC
and Windows SDK libraries. A normal Linux
`cargo build --target i686-pc-windows-msvc` has no MSVC linker or SDK, while the
default `cargo-xwin` architecture set does not include x86.

The command produces:

```text
target/i686-pc-windows-msvc/release/reforged_injector.exe
target/i686-pc-windows-msvc/release/reforged_sniffer.dll
```

## 2. Copy the artifacts into Guild Wars

For the normal edit-build-test loop, build and copy both artifacts beside
`Gw.exe` in one step:

```bash
bun run deploy:capture
```

The copy runs only after a successful build. Cargo retains its outputs in
`target`; the game directory receives disposable deployment copies.
A running client keeps its already loaded DLL; restart Guild Wars to test the
new build.

## 3. Launch Guild Wars before the injector

Steam expands `%command%` to Proton's normal `waitforexitandrun` invocation.
The local `steamarbitrarycommand.sh` helper replaces the original Windows
program after `waitforexitandrun` with the arguments after `--run`.
Consequently, passing `reforged_injector.exe` directly after `--run` would replace
`Gw.exe`; there would be no target process to inject.

Use a batch file to start the client first and inject after its process exists.
Create `reforged_sniffer_launcher.bat` in the Guild Wars installation directory,
usually:

```text
$HOME/.local/share/Steam/steamapps/common/Guild Wars/
```

Batch contents:

```bat
@echo off

cd /D "C:\Program Files (x86)\Guild Wars"
start "" "Gw.exe"

ping -n 15 127.0.0.1 > nul

".\reforged_injector.exe" Gw.exe ".\reforged_sniffer.dll"
```

Both artifacts are resolved relative to
`C:\Program Files (x86)\Guild Wars`, so this launcher does not use Wine's `Z:`
drive. The 15-ping delay follows the existing GWToolbox launcher pattern.
Increase it only if the injector reports `process not found: Gw.exe`.

Set the Steam launch options to:

```text
/home/USER/steamarbitrarycommand.sh game-performance %command% --run reforged_sniffer_launcher.bat
```

Replace `USER` in the Steam option with the Linux account name. The helper and
batch must remain readable and the helper must remain executable.

## 4. Verify the capture

With the DLL copied beside `Gw.exe`, each injection writes a new session below:

```text
$HOME/.local/share/Steam/steamapps/common/Guild Wars/captures/<session_id>/
```

`reforged_capture.jsonl` must report the installed hooks. For the current client,
a successful startup includes:

```text
hook_installed_text_decoder
hook_installed_stoc_handler_table
quest_info_request_ready
world_hooks_installed
vendor_hooks_installed
```

The same file periodically records `capture_health`. Nonzero drop or write
failure counters invalidate completeness claims for that session. Resource
records are written to the other session JSONL files only when the relevant
client events occur.

## 5. Verified environment

The copy-based path was validated on 2026-07-22 with Steam app `29720`, its
existing Guild Wars prefix, and `cachyos-11.0-20260602-slr`. Session
`1784745070077` loaded the DLL from the game directory, installed all five hook
groups listed above, and reported zero capture-health failures.

The validation client was started manually through Proton rather than through
Steam and became unresponsive. A second manual launch became unresponsive
before any injection and created no capture session. These observations verify
the relative DLL path and hook startup, but they neither demonstrate that
injection caused the freeze nor validate manual out-of-Steam launching. Use the
Steam launch option above for normal capture sessions.

The Steam argument rewrite was confirmed from the local
`steamarbitrarycommand.sh`; the batch sequence follows the existing working
GWToolbox launcher. Detailed runtime evidence is recorded in the
[investigation journal](../GWDAT_INVESTIGATION_JOURNAL.md).

Proton's official debugging guide documents `%command%` substitution and
running alternate Windows programs in a Steam compatibility environment:
[DEBUGGING-LINUX.md](https://github.com/ValveSoftware/Proton/blob/proton_11.0/docs/DEBUGGING-LINUX.md).
