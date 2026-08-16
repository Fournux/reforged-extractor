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

use super::text::{clean_item_name, is_invalid_label, looks_like_item_name};
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

pub(super) fn in_ranges(ordinal: u32, ranges: &[(u32, u32)]) -> bool {
    ranges
        .iter()
        .any(|&(start, end)| ordinal >= start && ordinal <= end)
}

pub(super) fn calc_runtime_ordinal_base(file_id: u32) -> u32 {
    let offset = file_id.saturating_sub(185423);
    let shift = if offset <= 26 {
        0
    } else if offset <= 50 {
        1
    } else {
        2
    };
    offset.saturating_sub(shift) * TEXT_RECORDS_PER_FILE
}

pub(super) fn build_item_name_catalog(
    archive: &mut DatArchive,
    pe: &PeImage<'_>,
) -> Result<BTreeMap<u32, BTreeMap<String, String>>> {
    let decoded_records = BTreeMap::new();
    let compact_seeds = BTreeMap::new();
    let mut reader = LocalizedTextReader::new(archive, pe, &compact_seeds, &decoded_records)?;
    let mut localized_names_by_id = BTreeMap::new();
    let mut seen = BTreeSet::new();

    for source in ITEM_TEXT_SOURCES {
        let Some(resource_file_id) = reader.file_id("en", source.text_file_index) else {
            continue;
        };
        let Some(entry_bytes) = reader.read_resource_file(resource_file_id)? else {
            continue;
        };
        let base_ordinal = calc_runtime_ordinal_base(resource_file_id);

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
            let name = clean_item_name(raw_name);
            if is_invalid_label(&name) {
                continue;
            }

            let string_id = base_ordinal + record.record_index;
            if !seen.insert((string_id, name.clone())) {
                continue;
            }

            let mut localized_name = reader
                .localized_record(source.text_file_index, record.record_index)?
                .into_iter()
                .filter_map(|(code, text)| {
                    let text = clean_item_name(&text);
                    (!text.is_empty()).then_some((code, text))
                })
                .collect::<BTreeMap<_, _>>();
            localized_name.insert("en".to_string(), name);
            localized_names_by_id.insert(string_id, localized_name);
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
    while offset + 8 <= raw_end {
        let model_file_id = u32::from_le_bytes([
            pe_bytes[offset],
            pe_bytes[offset + 1],
            pe_bytes[offset + 2],
            pe_bytes[offset + 3],
        ]);
        let name_string_id = u32::from_le_bytes([
            pe_bytes[offset + 4],
            pe_bytes[offset + 5],
            pe_bytes[offset + 6],
            pe_bytes[offset + 7],
        ]);

        if localized_names_by_id.contains_key(&name_string_id)
            && archive
                .mft_index_for_file_id(model_file_id)
                .is_some_and(|mft_index| archive.entry(mft_index).is_some())
        {
            candidate_starts.insert(offset);
        }
        offset += 4;
    }

    let run_len = |start: usize, stride: usize, candidate_starts: &BTreeSet<usize>| -> usize {
        let mut count = 0;
        let mut off = start;
        while candidate_starts.contains(&off) {
            count += 1;
            off += stride;
        }
        count
    };

    let mut covered = BTreeSet::new();
    let mut names_by_model_file_id =
        BTreeMap::<u32, BTreeMap<u32, BTreeMap<String, String>>>::new();

    for &start in &candidate_starts {
        if covered.contains(&start) {
            continue;
        }

        let len_24 = run_len(start, 0x24, &candidate_starts);
        let len_28 = run_len(start, 0x28, &candidate_starts);
        let (stride, count) = if len_28 >= len_24 {
            (0x28, len_28)
        } else {
            (0x24, len_24)
        };
        if count < 4 {
            continue;
        }

        for index in 0..count {
            let off = start + index * stride;
            covered.insert(off);
            let model_file_id = u32::from_le_bytes([
                pe_bytes[off],
                pe_bytes[off + 1],
                pe_bytes[off + 2],
                pe_bytes[off + 3],
            ]);
            let item_id = u32::from_le_bytes([
                pe_bytes[off + 4],
                pe_bytes[off + 5],
                pe_bytes[off + 6],
                pe_bytes[off + 7],
            ]);
            if let Some(names) = localized_names_by_id.get(&item_id) {
                names_by_model_file_id
                    .entry(model_file_id)
                    .or_default()
                    .insert(item_id, names.clone());
            }
        }
    }

    let mut out = BTreeMap::new();
    for (model_file_id, names_by_item_id) in names_by_model_file_id {
        let unique_names = names_by_item_id.values().cloned().collect::<BTreeSet<_>>();
        if unique_names.len() == 1
            && let Some(names) = unique_names.into_iter().next()
        {
            out.insert(model_file_id, names);
        }
    }
    out
}
