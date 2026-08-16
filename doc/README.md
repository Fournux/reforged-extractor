# Guild Wars Data Extraction Reference

Implementation-neutral specifications for reproducing confirmed Guild Wars client
data behavior from first-party files and observed official-client runtime state.
They define formats, joins, validation boundaries, and known limits; they do not
describe this repository's module layout, commands, output schema, or generated
corpus.

## Evidence and source policy

Use sources in this order:

1. `Gw.dat` and snapshots establish stored bytes, archive addressing, and
   localized resources.
2. The official client establishes how it locates, decodes, and joins those
   resources.
3. Narrow runtime capture is permitted only for relations not established by the
   primary files.

A rule is confirmed only by repeatable binary structure, client behavior, or
consistent independent runtime observations. A negative result rejects only the
tested representation; it does not prove that no encoded equivalent exists.
Unresolved joins remain unresolved rather than being inferred from proximity,
visual similarity, names, or counts.

## References

- [Archive container, MFT, file references, and localized text records](GWDAT_FORMAT.md)
- [Archive decompression and ATEX, ATTX, and DDS textures](DECOMPRESSION.md)
- [Skill tables, text, icons, and template-corpus boundary](SKILL_EXTRACTION.md)
- [Runtime item identity, localized text, and inventory icons](ITEM_EXTRACTION.md)
- [NPC identity, localized names, and runtime vendor services](NPC_AND_VENDOR_EXTRACTION.md)
- [Quest packets, localized text, dialogue roles, and rewards](QUEST_EXTRACTION.md)
- [Runtime capture under Steam and Proton](RUNTIME_CAPTURE.md)
