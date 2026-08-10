.PHONY: all build build-capture inject extract extract-all extract-skills extract-images extract-items extract-quests extract-npcs extract-vendors validate-items regen check test fmt help check-gw-dat check-capture-dir check-vendor-logs

PYTHON ?= python3
GW_DAT ?= $(HOME)/.local/share/Steam/steamapps/common/Guild Wars/Gw.dat
LATEST_CAPTURE_DIR := $(patsubst %/,%,$(lastword $(sort $(wildcard captures/[0-9]*/))))
CAPTURE_DIR ?= $(LATEST_CAPTURE_DIR)
CAPTURE_PATH = $(if $(wildcard $(CAPTURE_DIR)),$(CAPTURE_DIR),captures/$(CAPTURE_DIR))
EXTRACT = cargo run --release -p reforged-extractor -- extract
VENDOR_LOG_NAMES := reforged_npcs.jsonl reforged_vendor_context.jsonl reforged_collectors.jsonl reforged_merchants.jsonl reforged_crafters.jsonl reforged_skill_trainers.jsonl
VENDOR_ARGS = $(foreach log,$(VENDOR_LOGS),--packet-log "$(log)")

define require_capture_file
	@test -f "$(CAPTURE_PATH)/$(1)" || { echo "Required capture stream not found: $(CAPTURE_PATH)/$(1)" >&2; exit 1; }
endef

all: build

build:
	cargo build --release -p reforged-extractor

build-capture:
	cargo build --release --target i686-pc-windows-msvc -p reforged_sniffer -p reforged_injector

inject:
	target/i686-pc-windows-msvc/release/reforged_injector.exe Gw.exe target/i686-pc-windows-msvc/release/reforged_sniffer.dll

check-gw-dat:
	@test -f "$(GW_DAT)" || { echo "Gw.dat not found: $(GW_DAT) (set GW_DAT to use another path)" >&2; exit 1; }

check-capture-dir:
	@test -n "$(CAPTURE_DIR)" && test -d "$(CAPTURE_PATH)" || { echo "No capture found (set CAPTURE_DIR to a path or session ID)" >&2; exit 1; }

check-vendor-logs: check-capture-dir
	@test -n "$(strip $(VENDOR_LOGS))" || { echo "No vendor capture stream found in $(CAPTURE_PATH)" >&2; exit 1; }

extract: extract-all

extract-all:
	$(MAKE) extract-skills
	$(MAKE) extract-images
	$(MAKE) extract-items
	$(MAKE) extract-quests
	$(MAKE) extract-npcs
	$(MAKE) extract-vendors

extract-skills: check-gw-dat
	$(EXTRACT) skills --snapshot "$(GW_DAT)"

extract-images: check-gw-dat
	$(EXTRACT) images --snapshot "$(GW_DAT)"

extract-items: check-gw-dat check-capture-dir
	$(call require_capture_file,reforged_items.jsonl)
	$(EXTRACT) items --snapshot "$(GW_DAT)" --packet-log "$(CAPTURE_PATH)/reforged_items.jsonl"

extract-quests: check-gw-dat check-capture-dir
	$(call require_capture_file,reforged_npcs.jsonl)
	$(call require_capture_file,reforged_quests.jsonl)
	$(EXTRACT) quests --snapshot "$(GW_DAT)" \
		--packet-log "$(CAPTURE_PATH)/reforged_npcs.jsonl" \
		--packet-log "$(CAPTURE_PATH)/reforged_quests.jsonl" $(if $(wildcard $(CAPTURE_PATH)/reforged_items.jsonl),--item-log "$(CAPTURE_PATH)/reforged_items.jsonl")

extract-npcs: check-gw-dat check-capture-dir
	$(call require_capture_file,reforged_npcs.jsonl)
	$(EXTRACT) npcs --snapshot "$(GW_DAT)" \
		--packet-log "$(CAPTURE_PATH)/reforged_npcs.jsonl" $(if $(wildcard $(CAPTURE_PATH)/reforged_collectors.jsonl),--packet-log "$(CAPTURE_PATH)/reforged_collectors.jsonl")

extract-vendors: check-gw-dat check-vendor-logs
	$(EXTRACT) vendors --snapshot "$(GW_DAT)" $(VENDOR_ARGS)

validate-items:
	$(PYTHON) tools/validate_items_catalog.py output/items/items.json --require-complete

regen: extract-all
	$(MAKE) validate-items

check:
	cargo check --workspace

test:
	cargo test --workspace

fmt:
	cargo fmt --all

help:
	@echo "ReforgedExtractor Makefile targets:"
	@echo "  build           Build extractor CLI (release)"
	@echo "  build-capture   Build Win32 sniffer & injector"
	@echo "  inject          Run injector on Gw.exe"
	@echo "  extract-all     Run all extraction pipelines"
	@echo "  extract-skills  Extract skills catalog & icons"
	@echo "  extract-images  Extract map/UI images"
	@echo "  extract-items   Extract items (requires capture)"
	@echo "  extract-quests  Extract quests (requires capture)"
	@echo "  extract-npcs    Extract NPCs (requires capture)"
	@echo "  extract-vendors Extract vendors (requires capture)"
	@echo "  validate-items  Validate items output catalog"
	@echo "  regen           Run extract-all and validate-items"
	@echo "  check           Run cargo check"
	@echo "  test            Run cargo test"
	@echo "  fmt             Run cargo fmt"
	@echo ""
	@echo "Variables: GW_DAT=<path>, CAPTURE_DIR=<path-or-session-id>, PYTHON=<command>"
	@echo "Use make -n <target> for a dry run."
