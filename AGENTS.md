# Reforged Extractor

**Reforged Extractor** (`reforged-extractor`) reverse-engineers the official Guild Wars client data pipeline to build a structured, reproducible game database from local first-party files.

Technical findings about archive formats, decompression, resource indexing, string resolution, and runtime evidence belong in the focused references under [`doc/`](doc/README.md).

## 1. Mission

Extract and preserve as much game data and as many resources as possible from the local `Gw.dat` archive and local `.snapshot` files: skills, items, localized strings, images, models, metadata, and their relationships.

The intended result is an offline extractor that reproduces the official client's behavior for locating, decoding, decompressing, and resolving first-party data. It is not limited to any current resource or output.

## 2. Data Sources and Precedence

Use sources in this order:

1. **`Gw.dat` and `.snapshot` files first.** Extract required values and resources directly whenever possible. Determine whether the data is present and how it is encoded before relying on runtime data.
2. **The official client as the behavioral reference.** Its executable and runtime may be inspected, debugged, disassembled, or minimally instrumented to reproduce its indexing, decoding, decompression, and string-resolution behavior offline.
3. **Minimal official-client captures as a fallback.** When required values or relations cannot be extracted reliably from the primary files, sniff only the packet families and fields needed to supply them. Emit JSONL logs containing the minimum runtime evidence, join identifiers, and validation metadata.
4. **No external game data.** Do not fetch, scrape, or import game data from websites, public APIs, wikis, or third-party repositories.

JSONL captures are intermediate inputs, not final datasets. Cross-reference them with `Gw.dat` and `.snapshot` data to resolve strings, images, models, metadata, and other archive-backed values, then emit the deterministic final JSON required by the supported extractor. Do not capture or retain data that primary files can supply, except identifiers and provenance required for joins and validation.

## 3. Runtime Instrumentation Rules

* Keep hooks and packet logging minimal: observe only the code path, packet type, fields, and capture metadata required by the unresolved extraction.
* Prefer reproducing a client algorithm in the offline extractor over repeatedly calling the running client.
* Treat runtime observations as reverse-engineering evidence. Document the corresponding client behavior, file structure, or confirmed primary-file gap.
* Do not retain unrelated traffic or add speculative hooks.

## 4. Engineering Constraints

* Keep archive I/O, decompression, indexing, string resolution, resource-specific parsing, and output generation separated by responsibility.
* Extend existing patterns before adding abstractions. Support new resource types when their extraction is implemented, not through speculative scaffolding.
* Preserve unknown raw values when they may be needed for later interpretation; do not silently invent semantics.
* Use robust bounds checks and explicit errors for malformed offsets, blocks, indexes, and compressed streams.
* Optimize byte access and allocation only where extraction volume or measurements justify it.
* Code under `references/` is outdated. Use it only as conceptual evidence; never copy or directly translate it without validating the behavior against current first-party files and the official client.

## 5. Output Requirements

For every supported resource, follow its focused specification under [`doc/`](doc/README.md) and extract the relevant embedded identifiers, localized strings, file/model mappings, gameplay metadata, flags, and relationships.

Final JSON outputs must be deterministic and retain enough stable identifiers to join related resources.

## 6. Workflow

1. Define the required final JSON fields and inventory what `Gw.dat` and `.snapshot` contain; distinguish absent data from data whose encoding is not yet understood.
2. Trace the official client behavior needed to locate and decode those resources, then implement reliable direct extraction in the offline Rust pipeline whenever possible.
3. Only when required values or relations cannot be extracted directly, add the smallest runtime hook or packet capture and emit only the necessary JSONL evidence and join identifiers.
4. Cross-reference that JSONL evidence with `Gw.dat` and `.snapshot` data to resolve archive-backed strings, images, models, metadata, and relationships into the final JSON.
5. Record confirmed formats, algorithms, client behavior, and primary-file gaps in the appropriate focused reference under `doc/`.
6. Validate final outputs with exact counts, invariants, representative resources, and official-client behavior where applicable.
