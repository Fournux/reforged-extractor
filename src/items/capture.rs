use anyhow::Result;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use crate::{capture::for_each_jsonl_row, text::hex_to_bytes};

use super::text::{encoded_text_ids_from_hex, encoded_text_seeds_from_hex, is_invalid_label};
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct ItemObservation {
    pub(super) model_id: u32,
    pub(super) model_file_id: u32,
    pub(super) item_type: Option<u32>,
    pub(super) extra_id: Option<u32>,
    pub(super) materials: Option<u32>,
    pub(super) interaction: Option<u32>,
    pub(super) price: Option<u32>,
    pub(super) name_id: Option<u32>,
    pub(super) name_id_is_exact: bool,
    pub(super) enc_name_hex: Option<String>,
    pub(super) desc_enc_hex: Option<String>,
}
pub(super) type ItemKey = (u32, Option<u32>, Option<u32>);
pub(super) fn item_observation(decoded: &serde_json::Value) -> Option<ItemObservation> {
    let model_id = value_u32(decoded, "model_id")?;
    let model_file_id = value_model_file_id(decoded)?;
    let name_text_id = value_u32(decoded, "name_text_id");
    Some(ItemObservation {
        model_id,
        model_file_id,
        item_type: value_u32(decoded, "item_type").or_else(|| value_u32(decoded, "type")),
        extra_id: value_u32(decoded, "extra_id"),
        materials: value_u32(decoded, "materials"),
        interaction: value_u32(decoded, "interaction"),
        price: value_u32(decoded, "price"),
        name_id: name_text_id.or_else(|| value_u32(decoded, "name_id")),
        name_id_is_exact: name_text_id.is_some(),
        enc_name_hex: decoded
            .get("enc_name_hex")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        desc_enc_hex: decoded
            .get("desc_enc_hex")
            .or_else(|| decoded.get("description_enc_hex"))
            .and_then(|value| value.as_str())
            .map(str::to_string),
    })
}

pub(super) fn value_u32(value: &serde_json::Value, key: &str) -> Option<u32> {
    value
        .get(key)?
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
}

pub(super) fn value_model_file_id(value: &serde_json::Value) -> Option<u32> {
    value_u32(value, "model_file_id").map(|value| value & 0x7fff_ffff)
}

pub(super) fn item_key(row: &serde_json::Value) -> Option<ItemKey> {
    Some((
        value_u32(row, "item_id")?,
        value_u32(row, "model_id"),
        value_model_file_id(row),
    ))
}

pub(super) fn value_u32_array(value: &serde_json::Value, key: &str) -> Option<Vec<u32>> {
    value
        .get(key)?
        .as_array()?
        .iter()
        .map(|item| item.as_u64().and_then(|value| u32::try_from(value).ok()))
        .collect()
}

pub(super) fn insert_item_hex(
    row: &serde_json::Value,
    keys: &[&str],
    strings_by_item: &mut BTreeMap<ItemKey, String>,
) {
    let Some(key) = item_key(row) else {
        return;
    };
    let Some(hex) = keys
        .iter()
        .find_map(|key| row.get(*key).and_then(|value| value.as_str()))
        .filter(|hex| !hex.is_empty())
    else {
        return;
    };
    strings_by_item
        .entry(key)
        .or_insert_with(|| hex.to_string());
}

pub(super) fn insert_client_item_string(
    row: &serde_json::Value,
    source_key: &str,
    default_code: &str,
    strings_by_item: &mut BTreeMap<ItemKey, BTreeMap<String, String>>,
) -> bool {
    let Some(key) = item_key(row) else {
        return false;
    };
    let Some(text) = row
        .get(source_key)
        .and_then(|value| value.as_str())
        .map(crate::text::clean_display_text)
        .filter(|text| !text.is_empty() && !is_invalid_label(text))
    else {
        return false;
    };
    let code = row
        .get("lang")
        .and_then(|value| value.as_str())
        .unwrap_or(default_code);
    strings_by_item
        .entry(key)
        .or_default()
        .insert(code.to_string(), text);
    true
}
const ENCODED_TEXT_HEX_KEYS: &[&str] = &[
    "enc_name_hex",
    "complete_name_enc_hex",
    "desc_enc_hex",
    "description_enc_hex",
    "encoded_hex",
];

pub(super) fn insert_encoded_text_ids(text_ids: &mut BTreeSet<u32>, decoded: &serde_json::Value) {
    for key in ENCODED_TEXT_HEX_KEYS {
        if let Some(hex) = decoded.get(*key).and_then(|value| value.as_str())
            && let Some(ids) = encoded_text_ids_from_hex(hex)
        {
            text_ids.extend(ids);
        }
    }
}

pub(super) fn insert_encoded_text_seeds(
    seeds: &mut BTreeMap<u32, u64>,
    decoded: &serde_json::Value,
) {
    for key in ENCODED_TEXT_HEX_KEYS {
        if let Some(hex) = decoded.get(*key).and_then(|value| value.as_str())
            && let Some(parsed) = encoded_text_seeds_from_hex(hex)
        {
            seeds.extend(parsed);
        }
    }
}

pub(super) fn has_encoded_text_hex(decoded: &serde_json::Value) -> bool {
    ENCODED_TEXT_HEX_KEYS
        .iter()
        .any(|key| decoded.get(*key).is_some())
}

pub(super) fn insert_decoded_ids_by_encoded_hex(
    decoded_ids_by_encoded_hex: &mut BTreeMap<String, Vec<u32>>,
    decoded: &serde_json::Value,
) {
    let Some(ids) = value_u32_array(decoded, "decoded_ids") else {
        return;
    };
    for key in ENCODED_TEXT_HEX_KEYS {
        if let Some(hex) = decoded.get(*key).and_then(|value| value.as_str()) {
            decoded_ids_by_encoded_hex
                .entry(canonical_encoded_hex(hex))
                .or_insert_with(|| ids.clone());
        }
    }
}

pub(super) fn canonical_encoded_hex(hex: &str) -> String {
    let mut hex = hex.trim().to_ascii_lowercase();
    while hex.ends_with("0000") {
        hex.truncate(hex.len() - 4);
    }
    hex
}

pub(super) fn insert_text_decode_ids(text_ids: &mut BTreeSet<u32>, row: &serde_json::Value) {
    if row.get("kind").and_then(|kind| kind.as_str()) == Some("text_decode_ids") {
        if let Some(ids) = value_u32_array(row, "decoded_ids") {
            text_ids.extend(ids);
        }
        return;
    }
    let decoded = row.get("decoded").unwrap_or(row);
    if has_encoded_text_hex(decoded)
        && let Some(ids) = value_u32_array(decoded, "decoded_ids")
    {
        text_ids.extend(ids);
    }
}

#[derive(Debug, Default)]
pub(super) struct ItemCapture {
    pub(super) text_ids: BTreeSet<u32>,
    pub(super) compact_seeds: BTreeMap<u32, u64>,
    pub(super) decoded_records: BTreeMap<Vec<u8>, String>,
    pub(super) decoded_item_rows: usize,
    pub(super) observations: BTreeMap<ItemObservation, BTreeSet<u32>>,
    pub(super) client_names_by_item: BTreeMap<ItemKey, BTreeMap<String, String>>,
    pub(super) client_descriptions_by_item: BTreeMap<ItemKey, BTreeMap<String, String>>,
    pub(super) client_name_rows: usize,
    pub(super) client_description_rows: usize,
    pub(super) runtime_string_rows: usize,
    pub(super) decoded_ids_by_encoded_hex: BTreeMap<String, Vec<u32>>,
    pub(super) runtime_desc_hex_by_item: BTreeMap<ItemKey, String>,
    pub(super) runtime_name_hex_by_item: BTreeMap<ItemKey, String>,
    pub(super) runtime_desc_availability_by_item: BTreeMap<ItemKey, bool>,
}

pub(super) fn read_item_capture(
    packet_log_path: &Path,
    use_client_strings: bool,
) -> Result<ItemCapture> {
    let mut capture = ItemCapture::default();
    for_each_jsonl_row(packet_log_path, |_, row: serde_json::Value| {
        insert_text_decode_ids(&mut capture.text_ids, &row);
        let decoded = row.get("decoded").unwrap_or(&row);
        if let Some(text_id) =
            value_u32(decoded, "name_text_id").or_else(|| value_u32(decoded, "name_id"))
        {
            capture.text_ids.insert(text_id);
        }
        insert_encoded_text_ids(&mut capture.text_ids, decoded);
        insert_encoded_text_seeds(&mut capture.compact_seeds, &row);
        if let Some(decoded) = row.get("decoded") {
            insert_encoded_text_seeds(&mut capture.compact_seeds, decoded);
        }
        if use_client_strings {
            insert_decoded_text_record(&mut capture.decoded_records, &row);
        }

        match row.get("kind").and_then(|kind| kind.as_str()) {
            Some("text_decode_ids") => {
                if let (Some(hex), Some(ids)) = (
                    row.get("encoded_hex").and_then(|value| value.as_str()),
                    value_u32_array(&row, "decoded_ids"),
                ) {
                    capture
                        .decoded_ids_by_encoded_hex
                        .entry(canonical_encoded_hex(hex))
                        .or_insert(ids);
                }
                return Ok(());
            }
            Some("decoded_name") => {
                if use_client_strings
                    && insert_client_item_string(
                        &row,
                        "name",
                        "name",
                        &mut capture.client_names_by_item,
                    )
                {
                    capture.client_name_rows += 1;
                }
                return Ok(());
            }
            Some("decoded_description") => {
                if use_client_strings
                    && insert_client_item_string(
                        &row,
                        "description",
                        "description",
                        &mut capture.client_descriptions_by_item,
                    )
                {
                    capture.client_description_rows += 1;
                }
                return Ok(());
            }
            Some("runtime_item_strings") => {
                capture.runtime_string_rows += 1;
                insert_item_hex(
                    &row,
                    &["desc_enc_hex", "description_enc_hex"],
                    &mut capture.runtime_desc_hex_by_item,
                );
                insert_item_hex(
                    &row,
                    &["complete_name_enc_hex"],
                    &mut capture.runtime_name_hex_by_item,
                );
                if let Some(key) = item_key(&row) {
                    let available = row
                        .get("desc_complete")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or_else(|| {
                            row.get("desc_enc_hex")
                                .or_else(|| row.get("description_enc_hex"))
                                .and_then(serde_json::Value::as_str)
                                .is_some_and(|hex| !hex.is_empty())
                        });
                    capture
                        .runtime_desc_availability_by_item
                        .entry(key)
                        .and_modify(|current| *current |= available)
                        .or_insert(available);
                }
                insert_decoded_ids_by_encoded_hex(&mut capture.decoded_ids_by_encoded_hex, &row);
                return Ok(());
            }
            _ => {}
        }

        if use_client_strings {
            if insert_client_item_string(
                decoded,
                "decoded_name",
                "name",
                &mut capture.client_names_by_item,
            ) {
                capture.client_name_rows += 1;
            }
            if insert_client_item_string(
                decoded,
                "decoded_description",
                "description",
                &mut capture.client_descriptions_by_item,
            ) {
                capture.client_description_rows += 1;
            }
        }
        insert_decoded_ids_by_encoded_hex(&mut capture.decoded_ids_by_encoded_hex, decoded);
        let Some(observation) = item_observation(decoded) else {
            return Ok(());
        };
        capture.decoded_item_rows += 1;
        capture
            .observations
            .entry(observation)
            .or_default()
            .extend(value_u32(decoded, "item_id"));
        Ok(())
    })?;
    Ok(capture)
}

#[cfg(test)]
pub(crate) fn packet_log_text_ids(packet_log_path: &Path) -> Result<BTreeSet<u32>> {
    Ok(read_item_capture(packet_log_path, false)?.text_ids)
}

#[cfg(test)]
pub(crate) fn packet_log_text_seeds(packet_log_path: &Path) -> Result<BTreeMap<u32, u64>> {
    Ok(read_item_capture(packet_log_path, false)?.compact_seeds)
}

#[cfg(test)]
pub(crate) fn packet_log_decoded_text_records(
    packet_log_path: &Path,
) -> Result<BTreeMap<Vec<u8>, String>> {
    Ok(read_item_capture(packet_log_path, true)?.decoded_records)
}

pub(super) fn insert_decoded_text_record(
    records: &mut BTreeMap<Vec<u8>, String>,
    row: &serde_json::Value,
) {
    if row.get("kind").and_then(|kind| kind.as_str()) != Some("text_decode_trace") {
        return;
    }
    let (Some(record_hex), Some(text)) = (
        row.get("record_hex").and_then(|value| value.as_str()),
        row.get("output_preview").and_then(|value| value.as_str()),
    ) else {
        return;
    };
    let Some(record) = hex_to_bytes(record_hex) else {
        return;
    };
    match records.entry(record) {
        std::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(text.to_string());
        }
        std::collections::btree_map::Entry::Occupied(mut entry) => {
            if trace_text_score(text) > trace_text_score(entry.get()) {
                entry.insert(text.to_string());
            }
        }
    }
}

pub(super) fn trace_text_score(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    let printable = text.chars().filter(|ch| !ch.is_control()).count();
    if printable == 0 { 0 } else { printable + 1 }
}
