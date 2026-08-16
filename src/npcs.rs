use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use crate::{
    capture::for_each_jsonl_row,
    io_util::write_json,
    text::{
        catalog::{LocalizedTextCatalog, resolve_localized_text_catalog},
        clean_display_text, encoded_values_from_words, for_each_localized_reference, hex_to_bytes,
        text_references,
    },
};

#[derive(Debug, Deserialize)]
struct PacketRow {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    session_id: u64,
    #[serde(default)]
    header: u32,
    #[serde(default)]
    raw_hex: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct NpcDefinition {
    model_file_id: u32,
    skin_file_id: u32,
    visual_adjustment_raw: u32,
    appearance: u32,
    npc_flags: u32,
    primary_profession: u32,
    default_level: u32,
    name_words: Vec<u16>,
}

#[derive(Debug, Default)]
struct NpcAccumulator {
    definition: Option<NpcDefinition>,
    map_ids: BTreeSet<u32>,
    model_composites: BTreeSet<Vec<u32>>,
}

#[derive(Debug, Serialize)]
struct VisualAdjustment {
    hue: i8,
    saturation: i8,
    lightness: i8,
    scale_percent: u8,
}

#[derive(Debug, Serialize)]
struct NpcCatalogEntry {
    npc_model_id: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    model_file_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    skin_file_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    visual_adjustment_raw: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    visual_adjustment: Option<VisualAdjustment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    appearance: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    npc_flags: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    primary_profession: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_level: Option<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    map_ids: Vec<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    model_composites: Vec<Vec<u32>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    name_text_ids: Vec<u32>,
    #[serde(flatten)]
    localized: BTreeMap<String, String>,
}

pub(crate) fn extract_npcs_from_packet_log(
    gw_dat_path: &Path,
    packet_log_path: &Path,
    out_path: &Path,
) -> Result<()> {
    let npcs = read_npc_accumulators(packet_log_path)?;
    if npcs.is_empty() {
        bail!(
            "{} contains no decodable NPC observations",
            packet_log_path.display()
        );
    }

    let lookup = resolve_npc_text_catalog(gw_dat_path, &npcs)?;

    let catalog = npcs
        .into_iter()
        .map(|(npc_model_id, npc)| build_catalog_entry(npc_model_id, npc, &lookup))
        .collect::<Vec<_>>();
    write_json(out_path, &catalog)
}

pub(crate) fn resolve_localized_npc_names(
    gw_dat_path: &Path,
    encoded_names: &BTreeSet<Vec<u16>>,
) -> Result<BTreeMap<Vec<u16>, BTreeMap<String, String>>> {
    let references = encoded_names
        .iter()
        .map(|words| (words.clone(), npc_name_references(words)))
        .collect::<BTreeMap<_, _>>();
    let mut text_ids = BTreeSet::new();
    let mut compact_seeds = BTreeMap::new();
    for refs in references.values() {
        for text_ref in refs {
            text_ids.insert(text_ref.id);
            compact_seeds.entry(text_ref.id).or_insert(text_ref.seed);
        }
    }
    let lookup = resolve_localized_text_catalog(
        gw_dat_path,
        text_ids.iter().copied(),
        &compact_seeds,
        &BTreeMap::new(),
    )
    .with_context(|| format!("resolving NPC text from {}", gw_dat_path.display()))?;

    Ok(references
        .into_iter()
        .map(|(words, refs)| {
            let mut localized = BTreeMap::new();
            append_localized_name(&mut localized, &refs, &lookup);
            (words, localized)
        })
        .collect())
}

fn resolve_npc_text_catalog(
    gw_dat_path: &Path,
    npcs: &BTreeMap<u32, NpcAccumulator>,
) -> Result<LocalizedTextCatalog> {
    let mut text_ids = BTreeSet::new();
    let mut compact_seeds = BTreeMap::new();
    for npc in npcs.values() {
        let Some(definition) = &npc.definition else {
            continue;
        };
        for text_ref in npc_name_references(&definition.name_words) {
            text_ids.insert(text_ref.id);
            compact_seeds.entry(text_ref.id).or_insert(text_ref.seed);
        }
    }
    resolve_localized_text_catalog(
        gw_dat_path,
        text_ids.iter().copied(),
        &compact_seeds,
        &BTreeMap::new(),
    )
    .with_context(|| format!("resolving NPC text from {}", gw_dat_path.display()))
}

fn read_npc_accumulators(path: &Path) -> Result<BTreeMap<u32, NpcAccumulator>> {
    let mut npcs = BTreeMap::<u32, NpcAccumulator>::new();
    let mut current_maps = BTreeMap::<u64, u32>::new();

    for_each_jsonl_row(path, |_, row: PacketRow| {
        if row.kind != "world_packet" {
            return Ok(());
        }
        match row.header {
            0x0199 => {
                let bytes = packet_bytes(&row, 12)?;
                let map_id = u32_at(&bytes, 8).context("INSTANCE_LOAD_INFO map_id is truncated")?;
                if map_id == 0 {
                    current_maps.remove(&row.session_id);
                } else {
                    current_maps.insert(row.session_id, map_id);
                }
            }
            0x0020 => {
                let bytes = packet_bytes(&row, 12)?;
                let agent_type =
                    u32_at(&bytes, 8).context("AGENT_SPAWNED agent_type is truncated")?;
                if agent_type & 0xf000_0000 == 0x2000_0000 {
                    let npc_model_id = agent_type & 0x0fff_ffff;
                    let npc = npcs.entry(npc_model_id).or_default();
                    if let Some(map_id) = current_maps.get(&row.session_id) {
                        npc.map_ids.insert(*map_id);
                    }
                }
            }
            0x0056 => {
                let bytes = packet_bytes(&row, 0x34)?;
                let npc_model_id =
                    u32_at(&bytes, 4).context("NPC_UPDATE_PROPERTIES npc_model_id is truncated")?;
                let definition = NpcDefinition {
                    model_file_id: required_u32(&bytes, 8, "model_file_id")?,
                    skin_file_id: required_u32(&bytes, 12, "skin_file_id")?,
                    visual_adjustment_raw: required_u32(&bytes, 16, "visual_adjustment")?,
                    appearance: required_u32(&bytes, 20, "appearance")?,
                    npc_flags: required_u32(&bytes, 24, "npc_flags")?,
                    primary_profession: required_u32(&bytes, 28, "primary_profession")?,
                    default_level: required_u32(&bytes, 32, "default_level")?,
                    name_words: fixed_words(&bytes, 36, 8),
                };
                let npc = npcs.entry(npc_model_id).or_default();
                if npc
                    .definition
                    .as_ref()
                    .is_some_and(|current| current != &definition)
                {
                    bail!("NPC model {npc_model_id} has conflicting property observations");
                }
                npc.definition = Some(definition);
            }
            0x0057 => {
                let bytes = packet_bytes(&row, 12)?;
                let npc_model_id = required_u32(&bytes, 4, "npc_model_id")?;
                let count = required_u32(&bytes, 8, "composite_count")? as usize;
                if count > 8 || bytes.len() < 12 + count * 4 {
                    bail!("NPC model {npc_model_id} has an invalid model composite");
                }
                let model_files = (0..count)
                    .map(|index| required_u32(&bytes, 12 + index * 4, "model_file"))
                    .collect::<Result<Vec<_>>>()?;
                npcs.entry(npc_model_id)
                    .or_default()
                    .model_composites
                    .insert(model_files);
            }
            _ => {}
        }
        Ok(())
    })?;
    Ok(npcs)
}

fn build_catalog_entry(
    npc_model_id: u32,
    npc: NpcAccumulator,
    lookup: &LocalizedTextCatalog,
) -> NpcCatalogEntry {
    let mut localized = BTreeMap::new();
    let mut name_text_ids = Vec::new();
    if let Some(definition) = &npc.definition {
        let refs = npc_name_references(&definition.name_words);
        name_text_ids = refs.iter().map(|text_ref| text_ref.id).collect();
        append_localized_name(&mut localized, &refs, lookup);
    }
    let visual_adjustment = npc.definition.as_ref().map(|definition| {
        let bytes = definition.visual_adjustment_raw.to_le_bytes();
        VisualAdjustment {
            hue: bytes[0] as i8,
            saturation: bytes[1] as i8,
            lightness: bytes[2] as i8,
            scale_percent: bytes[3],
        }
    });

    NpcCatalogEntry {
        npc_model_id,
        model_file_id: npc.definition.as_ref().map(|value| value.model_file_id),
        skin_file_id: npc.definition.as_ref().map(|value| value.skin_file_id),
        visual_adjustment_raw: npc
            .definition
            .as_ref()
            .map(|value| value.visual_adjustment_raw),
        visual_adjustment,
        appearance: npc.definition.as_ref().map(|value| value.appearance),
        npc_flags: npc.definition.as_ref().map(|value| value.npc_flags),
        primary_profession: npc
            .definition
            .as_ref()
            .map(|value| value.primary_profession),
        default_level: npc.definition.as_ref().map(|value| value.default_level),
        map_ids: npc.map_ids.into_iter().collect(),
        model_composites: npc.model_composites.into_iter().collect(),
        name_text_ids,
        localized,
    }
}

fn npc_name_references(words: &[u16]) -> Vec<crate::text::TextReference> {
    let references = text_references(words);
    if !references.is_empty() {
        return references;
    }
    let Some(values) = encoded_values_from_words(words) else {
        return references;
    };
    let [(value, start, end)] = values.as_slice() else {
        return references;
    };
    let Ok(id) = u32::try_from(*value) else {
        return references;
    };
    if id < 1024 || *start != 0 || *end != words.len() {
        return references;
    }
    vec![crate::text::TextReference {
        id,
        seed: 0,
        start: *start,
        end: *end,
    }]
}

fn append_localized_name(
    out: &mut BTreeMap<String, String>,
    refs: &[crate::text::TextReference],
    lookup: &LocalizedTextCatalog,
) {
    for_each_localized_reference(refs, lookup, |code, text| {
        let text = clean_display_text(text);
        if !text.is_empty() {
            out.insert(format!("name_{code}"), text);
        }
    });
}

fn packet_bytes(row: &PacketRow, minimum_len: usize) -> Result<Vec<u8>> {
    let bytes = hex_to_bytes(&row.raw_hex).context("packet raw_hex is invalid")?;
    if bytes.len() < minimum_len || u32_at(&bytes, 0) != Some(row.header) {
        bail!(
            "packet 0x{:04X} is truncated or has a mismatched header",
            row.header
        );
    }
    Ok(bytes)
}

fn required_u32(bytes: &[u8], offset: usize, field: &str) -> Result<u32> {
    u32_at(bytes, offset).with_context(|| format!("NPC {field} is truncated"))
}

fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    let bytes = bytes.get(offset..offset + 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn fixed_words(bytes: &[u8], offset: usize, capacity: usize) -> Vec<u16> {
    bytes
        .get(offset..offset + capacity * 2)
        .unwrap_or_default()
        .chunks_exact(2)
        .map(|word| u16::from_le_bytes([word[0], word[1]]))
        .take_while(|word| *word != 0)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::TestDir;

    fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn packet_row(header: u32, bytes: &[u8]) -> String {
        serde_json::json!({
            "kind": "world_packet",
            "session_id": 7,
            "header": header,
            "raw_hex": hex::encode(bytes),
        })
        .to_string()
    }

    #[test]
    fn npc_capture_aggregates_definition_map_model_and_direct_name() -> Result<()> {
        let mut load = [0_u8; 0x1c];
        put_u32(&mut load, 0, 0x199);
        put_u32(&mut load, 8, 146);

        let mut properties = [0_u8; 0x34];
        put_u32(&mut properties, 0, 0x56);
        put_u32(&mut properties, 4, 525);
        put_u32(&mut properties, 8, 346_556);
        put_u32(&mut properties, 12, 7);
        put_u32(&mut properties, 16, 0x6400_0000);
        put_u32(&mut properties, 20, 1);
        put_u32(&mut properties, 24, 524);
        put_u32(&mut properties, 28, 3);
        put_u32(&mut properties, 32, 20);
        properties[36..40].copy_from_slice(&[0x8102_u16, 0x465b].map(u16::to_le_bytes).concat());

        let mut spawned = [0_u8; 0x74];
        put_u32(&mut spawned, 0, 0x20);
        put_u32(&mut spawned, 4, 42);
        put_u32(&mut spawned, 8, 0x2000_0000 | 525);

        let mut model = [0_u8; 0x2c];
        put_u32(&mut model, 0, 0x57);
        put_u32(&mut model, 4, 525);
        put_u32(&mut model, 8, 2);
        put_u32(&mut model, 12, 111);
        put_u32(&mut model, 16, 222);

        let temp = TestDir::new()?;
        let path = temp.path().join("npcs.jsonl");
        std::fs::write(
            &path,
            [
                packet_row(0x199, &load),
                packet_row(0x56, &properties),
                packet_row(0x20, &spawned),
                packet_row(0x57, &model),
            ]
            .join("\n"),
        )?;

        let npcs = read_npc_accumulators(&path)?;
        let npc = npcs.get(&525).context("NPC 525 missing")?;
        let definition = npc.definition.as_ref().context("NPC definition missing")?;
        assert_eq!(definition.model_file_id, 346_556);
        assert_eq!(definition.skin_file_id, 7);
        assert_eq!(npc.map_ids, BTreeSet::from([146]));
        assert_eq!(npc.model_composites, BTreeSet::from([vec![111, 222]]));
        assert_eq!(npc_name_references(&definition.name_words)[0].id, 82_779);
        assert_eq!(
            clean_display_text("[F]Agent[f:\"Agentka\"] [lbracket]Storage[rbracket]"),
            "Agent [Storage]"
        );
        Ok(())
    }

    #[test]
    fn zero_map_clears_current_npc_map() -> Result<()> {
        let mut load = [0_u8; 0x1c];
        put_u32(&mut load, 0, 0x0199);
        put_u32(&mut load, 8, 146);
        let mut clear = load;
        put_u32(&mut clear, 8, 0);

        let mut first_spawn = [0_u8; 0x74];
        put_u32(&mut first_spawn, 0, 0x0020);
        put_u32(&mut first_spawn, 8, 0x2000_0000 | 525);
        let mut second_spawn = first_spawn;
        put_u32(&mut second_spawn, 8, 0x2000_0000 | 526);

        let temp = TestDir::new()?;
        let path = temp.path().join("npcs.jsonl");
        std::fs::write(
            &path,
            [
                packet_row(0x0199, &load),
                packet_row(0x0020, &first_spawn),
                packet_row(0x0199, &clear),
                packet_row(0x0020, &second_spawn),
            ]
            .join("\n"),
        )?;

        let npcs = read_npc_accumulators(&path)?;
        assert_eq!(
            npcs.get(&525).context("NPC 525 missing")?.map_ids,
            BTreeSet::from([146])
        );
        assert!(
            npcs.get(&526)
                .context("NPC 526 missing")?
                .map_ids
                .is_empty()
        );
        Ok(())
    }
}
