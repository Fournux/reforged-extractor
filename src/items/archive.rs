use anyhow::{Context, Result};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use crate::{
    dat::DatArchive,
    pe::PeImage,
    text::{
        catalog::{LocalizedTextReader, resolve_localized_text_catalog_with_client},
        records::{self, TEXT_RECORDS_PER_FILE},
    },
};

use super::text::{is_invalid_label, looks_like_item_name};
#[derive(Debug, Default)]
pub(super) struct ItemTextLookup {
    pub(super) by_text_id: BTreeMap<u32, BTreeMap<String, String>>,
    pub(super) by_model_file_id: BTreeMap<u32, BTreeMap<String, String>>,
    pub(super) exact_text_ids: BTreeSet<u32>,
}
struct ItemTextSource {
    text_file_index: usize,
    ranges: &'static [(u32, u32)],
}

const ITEM_TEXT_SOURCES: &[ItemTextSource] = &[
    ItemTextSource {
        text_file_index: 2,
        ranges: &[(364, 399)], // common crafting materials
    },
    ItemTextSource {
        text_file_index: 8,
        ranges: &[(1, 33)], // early weapons and upgrades
    },
    ItemTextSource {
        text_file_index: 9,
        ranges: &[(0, 488)], // Prophecies weapons and armor
    },
    ItemTextSource {
        text_file_index: 10,
        ranges: &[(0, 119)], // trophies and collectibles
    },
    ItemTextSource {
        text_file_index: 20,
        ranges: &[(2, 127)], // Factions armor
    },
    ItemTextSource {
        text_file_index: 21,
        ranges: &[(0, 223)], // Factions armor, weapons, and upgrades
    },
    ItemTextSource {
        text_file_index: 27,
        ranges: &[(72, 86), (99, 375)], // unique weapons and Obsidian armor
    },
    ItemTextSource {
        text_file_index: 28,
        ranges: &[(0, 1023)], // Factions armor continuation
    },
    ItemTextSource {
        text_file_index: 29,
        ranges: &[(0, 811)], // Factions armor continuation
    },
    ItemTextSource {
        text_file_index: 31,
        ranges: &[(2, 10)], // starter weapons
    },
    ItemTextSource {
        text_file_index: 48,
        ranges: &[(0, 19)], // inscriptions and attribute scrolls
    },
    ItemTextSource {
        text_file_index: 56,
        ranges: &[(4, 4), (153, 154)], // holiday weapons
    },
    ItemTextSource {
        text_file_index: 63,
        ranges: &[(1, 2)], // Birthday Cupcake
    },
    ItemTextSource {
        text_file_index: 65,
        ranges: &[(0, 962)], // Eye of the North weapons and armor
    },
    ItemTextSource {
        text_file_index: 66,
        ranges: &[(0, 521)], // Eye of the North armor and miniatures
    },
    ItemTextSource {
        text_file_index: 67,
        ranges: &[(0, 0), (108, 122)], // Eye of the North quest and weapon tail
    },
    ItemTextSource {
        text_file_index: 70,
        ranges: &[(3, 4)], // dungeon maps and ale
    },
    ItemTextSource {
        text_file_index: 80,
        ranges: &[(0, 39)], // tournament tokens
    },
    ItemTextSource {
        text_file_index: 83,
        ranges: &[(1, 6)], // Zaishen coins
    },
    ItemTextSource {
        text_file_index: 85,
        ranges: &[(32, 36)], // store service items
    },
    ItemTextSource {
        text_file_index: 89,
        ranges: &[(0, 18)], // miniatures and White Mantle weapons
    },
    ItemTextSource {
        text_file_index: 94,
        ranges: &[(0, 28), (102, 118), (244, 244)], // Winds of Change
    },
];

const MODEL_NAME_LINK_PREFIX_SIZE: usize = 8;
const MODEL_NAME_LINK_SCAN_STEP: usize = 4;
const MODEL_NAME_LINK_COMPACT_STRIDE: usize = 0x24;
const MODEL_NAME_LINK_EXTENDED_STRIDE: usize = 0x28;
const MIN_MODEL_NAME_LINK_RUN: usize = 4;

fn in_ranges(ordinal: u32, ranges: &[(u32, u32)]) -> bool {
    ranges
        .iter()
        .any(|&(start, end)| ordinal >= start && ordinal <= end)
}

pub(super) fn build_item_name_catalog(
    archive: &mut DatArchive,
    pe: &PeImage<'_>,
) -> Result<BTreeMap<u32, BTreeMap<String, String>>> {
    let decoded_records = BTreeMap::new();
    let compact_seeds = BTreeMap::new();
    let mut reader = LocalizedTextReader::new(archive, pe, &compact_seeds, &decoded_records)?;
    let mut localized_names_by_id = BTreeMap::new();

    for source in ITEM_TEXT_SOURCES {
        let Some(resource_file_id) = reader.file_id("en", source.text_file_index) else {
            continue;
        };
        let Some(entry_bytes) = reader.read_resource_file(resource_file_id)? else {
            continue;
        };
        let base_text_id = u32::try_from(source.text_file_index)
            .context("item text file index exceeds u32")?
            .checked_mul(TEXT_RECORDS_PER_FILE)
            .context("item text id base overflow")?;

        for record in records::parse_text_record_entries(&entry_bytes).with_context(|| {
            format!("parsing item text records from DAT file {resource_file_id}")
        })? {
            if !in_ranges(record.ordinal, source.ranges) {
                continue;
            }
            let raw_name = record
                .text
                .trim_end_matches('\0')
                .trim_start_matches('\u{feff}');
            if !looks_like_item_name(raw_name) {
                continue;
            }
            let name = crate::text::clean_display_text(raw_name);
            if is_invalid_label(&name) {
                continue;
            }

            let text_id = base_text_id
                .checked_add(record.record_index)
                .context("item text id overflow")?;

            let mut localized_name = reader
                .localized_record(source.text_file_index, record.record_index)?
                .into_iter()
                .filter_map(|(code, text)| {
                    let text = crate::text::clean_display_text(&text);
                    (!text.is_empty()).then_some((code, text))
                })
                .collect::<BTreeMap<_, _>>();
            localized_name.insert("en".to_string(), name);
            localized_names_by_id.insert(text_id, localized_name);
        }
    }

    Ok(localized_names_by_id)
}
pub(super) fn resolve_item_text_lookup(
    gw_dat_path: &Path,
    text_ids: &BTreeSet<u32>,
    compact_seeds: &BTreeMap<u32, u64>,
    decoded_records: &BTreeMap<Vec<u8>, String>,
) -> Result<ItemTextLookup> {
    let mut archive = DatArchive::open(gw_dat_path)?;
    let pe_data = archive.client_pe_data()?;
    let pe = PeImage::parse(&pe_data)?;
    let mut by_text_id = build_item_name_catalog(&mut archive, &pe)?;
    let by_model_file_id = scan_model_file_simple_name_links(&pe, &by_text_id, &archive);
    let requested = resolve_localized_text_catalog_with_client(
        &mut archive,
        &pe,
        text_ids.iter().copied(),
        compact_seeds,
        decoded_records,
    )?;
    let exact_text_ids = requested.by_text_id.keys().copied().collect();
    for (text_id, localized) in requested.by_text_id {
        by_text_id.entry(text_id).or_insert(localized);
    }
    Ok(ItemTextLookup {
        by_text_id,
        by_model_file_id,
        exact_text_ids,
    })
}
fn candidate_run_len(start: usize, stride: usize, candidate_starts: &BTreeSet<usize>) -> usize {
    let mut count = 0;
    let mut offset = start;
    while candidate_starts.contains(&offset) {
        count += 1;
        let Some(next) = offset.checked_add(stride) else {
            break;
        };
        offset = next;
    }
    count
}

fn model_name_link_layout(
    start: usize,
    candidate_starts: &BTreeSet<usize>,
) -> Option<(usize, usize)> {
    let compact_len = candidate_run_len(start, MODEL_NAME_LINK_COMPACT_STRIDE, candidate_starts);
    let extended_len = candidate_run_len(start, MODEL_NAME_LINK_EXTENDED_STRIDE, candidate_starts);
    let layout = if extended_len >= compact_len {
        (MODEL_NAME_LINK_EXTENDED_STRIDE, extended_len)
    } else {
        (MODEL_NAME_LINK_COMPACT_STRIDE, compact_len)
    };
    (layout.1 >= MIN_MODEL_NAME_LINK_RUN).then_some(layout)
}

pub(super) fn scan_model_file_simple_name_links(
    pe: &PeImage<'_>,
    localized_names_by_id: &BTreeMap<u32, BTreeMap<String, String>>,
    archive: &DatArchive,
) -> BTreeMap<u32, BTreeMap<String, String>> {
    let pe_bytes = pe.data();
    let Some(rdata) = pe
        .sections()
        .iter()
        .find(|section| section.name == ".rdata")
    else {
        return BTreeMap::new();
    };
    let raw_range = rdata.raw_range();
    let raw_start = raw_range.start;
    let raw_end = raw_range.end;

    let mut candidate_starts = BTreeSet::new();
    let mut offset = raw_start;
    while offset
        .checked_add(MODEL_NAME_LINK_PREFIX_SIZE)
        .is_some_and(|end| end <= raw_end)
    {
        let model_file_id = u32::from_le_bytes([
            pe_bytes[offset],
            pe_bytes[offset + 1],
            pe_bytes[offset + 2],
            pe_bytes[offset + 3],
        ]);
        let name_text_id = u32::from_le_bytes([
            pe_bytes[offset + 4],
            pe_bytes[offset + 5],
            pe_bytes[offset + 6],
            pe_bytes[offset + 7],
        ]);

        if localized_names_by_id.contains_key(&name_text_id)
            && archive.entry_for_file_id(model_file_id).is_some()
        {
            candidate_starts.insert(offset);
        }
        offset += MODEL_NAME_LINK_SCAN_STEP;
    }

    let mut covered = BTreeSet::new();
    let mut text_ids_by_model_file_id = BTreeMap::<u32, BTreeSet<u32>>::new();
    for &start in &candidate_starts {
        if covered.contains(&start) {
            continue;
        }
        let Some((stride, count)) = model_name_link_layout(start, &candidate_starts) else {
            continue;
        };

        for index in 0..count {
            let offset = start + index * stride;
            covered.insert(offset);
            let model_file_id = u32::from_le_bytes([
                pe_bytes[offset],
                pe_bytes[offset + 1],
                pe_bytes[offset + 2],
                pe_bytes[offset + 3],
            ]);
            let name_text_id = u32::from_le_bytes([
                pe_bytes[offset + 4],
                pe_bytes[offset + 5],
                pe_bytes[offset + 6],
                pe_bytes[offset + 7],
            ]);
            if localized_names_by_id.contains_key(&name_text_id) {
                text_ids_by_model_file_id
                    .entry(model_file_id)
                    .or_default()
                    .insert(name_text_id);
            }
        }
    }

    let mut out = BTreeMap::new();
    for (model_file_id, text_ids) in text_ids_by_model_file_id {
        let mut localized = text_ids
            .iter()
            .filter_map(|text_id| localized_names_by_id.get(text_id));
        let Some(first) = localized.next() else {
            continue;
        };
        if localized.all(|names| names == first) {
            out.insert(model_file_id, first.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate_run(stride: usize, count: usize) -> BTreeSet<usize> {
        (0..count).map(|index| 100 + index * stride).collect()
    }

    #[test]
    fn item_text_sources_have_unique_valid_ranges() {
        let mut file_indices = BTreeSet::new();
        for source in ITEM_TEXT_SOURCES {
            assert!(file_indices.insert(source.text_file_index));
            assert!(!source.ranges.is_empty());
            for &(start, end) in source.ranges {
                assert!(start <= end);
                assert!(end < TEXT_RECORDS_PER_FILE);
            }
        }
    }

    #[test]
    fn model_name_link_layout_requires_a_complete_run() {
        let compact = candidate_run(MODEL_NAME_LINK_COMPACT_STRIDE, MIN_MODEL_NAME_LINK_RUN);
        assert_eq!(
            model_name_link_layout(100, &compact),
            Some((MODEL_NAME_LINK_COMPACT_STRIDE, MIN_MODEL_NAME_LINK_RUN))
        );

        let short = candidate_run(MODEL_NAME_LINK_EXTENDED_STRIDE, MIN_MODEL_NAME_LINK_RUN - 1);
        assert_eq!(model_name_link_layout(100, &short), None);
    }

    #[test]
    fn model_name_link_layout_prefers_extended_stride_on_ties() {
        let mut candidates = candidate_run(MODEL_NAME_LINK_COMPACT_STRIDE, MIN_MODEL_NAME_LINK_RUN);
        candidates.extend(candidate_run(
            MODEL_NAME_LINK_EXTENDED_STRIDE,
            MIN_MODEL_NAME_LINK_RUN,
        ));

        assert_eq!(
            model_name_link_layout(100, &candidates),
            Some((MODEL_NAME_LINK_EXTENDED_STRIDE, MIN_MODEL_NAME_LINK_RUN))
        );
    }
}
