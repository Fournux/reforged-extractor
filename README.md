<div align="center">
  <img src="./logo.png" alt="TyriaExtractor" width="600">
  <p><strong>A Rust toolkit for extracting structured Guild Wars game data.</strong></p>
  <p>
    <img alt="Rust 1.97.1" src="https://img.shields.io/badge/Rust-1.97.0-000000?logo=rust&amp;logoColor=white">
    <img alt="MIT license" src="https://img.shields.io/badge/license-MIT-blue">
  </p>
</div>

## Quick Start

### 1. Build

```bash
# Build the native extractor
cargo build --release -p tyria-extractor-rs

# (Windows optional) Build sniffer & injector for live runtime captures
rustup target add i686-pc-windows-msvc
cargo build --release --target i686-pc-windows-msvc -p tyria_injector -p tyria_sniffer
```

### 2. Extract Data from `Gw.dat`

Extract skills and images directly from your local `Gw.dat`:

```bash
cargo run --release -- extract skills --snapshot "C:\path\to\Gw.dat"
cargo run --release -- extract images --snapshot "C:\path\to\Gw.dat"
```

### 3. Extract Runtime Data (Items, NPCs, Quests, Vendors)

For datasets requiring runtime joins, inject the sniffer while the client is running:

```powershell
.\target\i686-pc-windows-msvc\release\tyria_injector.exe Gw.exe .\target\i686-pc-windows-msvc\release\tyria_sniffer.dll
```

Then extract using the generated capture logs:

```powershell
cargo run --release -- extract items --snapshot "C:\path\to\Gw.dat" --packet-log ".\captures\<session-id>\tyria_items.jsonl"
cargo run --release -- extract npcs --snapshot "C:\path\to\Gw.dat" --packet-log ".\captures\<session-id>\tyria_npcs.jsonl"
cargo run --release -- extract quests --snapshot "C:\path\to\Gw.dat" --packet-log ".\captures\<session-id>\tyria_quests.jsonl" --item-log ".\captures\<session-id>\tyria_items.jsonl"
cargo run --release -- extract vendors --snapshot "C:\path\to\Gw.dat" --packet-log ".\captures\<session-id>\tyria_vendor_context.jsonl"
```

_(Or use `make regen` / `make extract-vendors` to run automatically with the latest capture)._

## Output Structure

All outputs are saved in `output/`:

```text
output/
├── skills/       # skills.json, model_file/*.png
├── images/       # manifest.json, png/*.png
├── items/        # items.json, model_file/*.png
├── npcs/         # npcs.json
├── quests/       # quests.json
└── vendors/      # collectors.json, merchants.json, crafters.json, etc.
```

## Documentation

- [DAT Format Specification](doc/GWDAT_FORMAT.md)
- [ATEX & Decompression](doc/DECOMPRESSION.md)
- [Skill Extraction](doc/SKILL_EXTRACTION.md)
- [Item Extraction](doc/ITEM_EXTRACTION.md)
- [NPC & Vendor Extraction](doc/NPC_AND_VENDOR_EXTRACTION.md)
- [Quest Extraction](doc/QUEST_EXTRACTION.md)
- [Investigation Journal](GWDAT_INVESTIGATION_JOURNAL.md)

## Legal

TyriaExtractor is an unofficial, independent tool. Guild Wars and all game assets are property of ArenaNet / NCSOFT. Software licensed under the [MIT License](LICENSE).
