# Runtime Capture on Linux with Proton

This reference describes the minimum conditions for observing official-client
runtime state from a 32-bit Guild Wars client under Steam Proton. It is
implementation-neutral: `<injector>.exe`, `<capture>.dll`, and the capture
format below stand for an observer's own artifacts and data contract.

## 1. Process boundary

The injector and capture DLL must run as 32-bit Windows binaries in the same
Proton prefix as the official `Gw.exe` process. A native Linux injector, a
different Wine prefix, or injection before `Gw.exe` exists cannot reach that
process.

## 2. Build 32-bit Windows artifacts

For a Rust implementation, install the target and
[`cargo-xwin`](https://github.com/rust-cross/cargo-xwin) once:

```bash
rustup target add i686-pc-windows-msvc
cargo install --locked cargo-xwin
```

Build the injector and DLL for x86:

```bash
XWIN_ARCH=x86 cargo xwin build --release --target i686-pc-windows-msvc
```

The manifest must select the binary and DLL packages appropriate to the
implementation. `XWIN_ARCH=x86` is required: a normal Linux Cargo build has no
MSVC linker or SDK, and `cargo-xwin` does not necessarily download x86 import
libraries by default.

## 3. Launch order

Copy the generated Windows artifacts beside `Gw.exe`. A launcher started by
Steam's normal Proton environment must:

1. start `Gw.exe`;
2. wait until the process exists;
3. inject the DLL into that process.

For example, a batch launcher in the game directory can use:

```bat
@echo off
cd /D "<Guild Wars directory>"
start "" "Gw.exe"
ping -n 15 127.0.0.1 > nul
"<injector>.exe" Gw.exe "<capture>.dll"
```

The delay is only a process-creation wait. Increase it only when the injector
reports that `Gw.exe` does not exist yet.

Some Steam wrappers replace the Windows program passed after their `--run`
argument. With such a wrapper, passing `<injector>.exe` directly would replace
`Gw.exe` rather than inject into it; pass the batch launcher instead. The exact
Steam launch-option syntax and compatibility-tool selection are local setup
details, but `%command%` must expand to Steam's normal Proton launch command.

## 4. Capture evidence contract

Write each capture to its own session directory. Keep metadata separate from
domain data:

| Data | Required content |
| --- | --- |
| Capture sidecar | Client-build identity, installed hook groups, packet schemas and expected sizes, and capture-health counters |
| Domain streams | Only their observed item, quest, NPC, dialogue, or service rows |

Every cross-domain row needs a session identifier and one session-monotonic
sequence number shared by all streams. Consumers merge by that pair, not input
file order, so agent lifetime, map transitions, dialogue, item, and service
relations retain their observed order.

A session is usable only when:

- every consumed packet family has a recorded schema whose descriptors agree
  with its expected size;
- its client-build metadata is internally consistent;
- sequence numbers are unique and gap-free across the supplied streams; and
- lock drops, capacity evictions, and write failures are all zero.

Do not mix health or hook metadata into domain streams, and do not treat an
absent health record as zero loss. Reject incomplete or unhealthy evidence
instead of emitting partial semantic claims.

## 5. Startup verification

Before relying on a session, verify that the sidecar records successful
installation of every hook group used by the extraction. A complete
quest/NPC/vendor observer normally needs the text decoder, packet-handler,
quest-info request, world-packet, and vendor-hook groups.

Then verify the emitted data rather than hook installation alone:

1. each required packet family occurs with its declared size;
2. agent despawns and map-load contexts are present when their joins are used;
3. active quests observed before injection receive one normal quest-description
   response through the official quest-info request; and
4. capture-health counters remain zero through the session.

Launching Guild Wars manually outside Steam's usual path is not a valid
responsiveness test. A freeze observed before injection does not establish that
injection caused it; use the normal Steam launch environment to test behavior.
