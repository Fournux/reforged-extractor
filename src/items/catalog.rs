use anyhow::Result;
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use crate::{io_util::write_json, text::clean_display_text};

use super::{
    archive::ItemTextLookup,
    capture::{ItemCapture, ItemKey, ItemObservation, canonical_encoded_hex},
    text::{
        GENERIC_ITEM_NAME_TEXT_ID, asyncdecode_item_ids_from_hex,
        decode_description_fields_from_exact_ids, decode_encoded_description_fields,
        decode_encoded_name_fields, decode_name_fields_from_exact_ids, encoded_text_ids_from_hex,
        flat_runtime_name_fields, flat_runtime_text_fields, is_invalid_label,
    },
};

#[cfg(test)]
use super::capture::read_item_capture;

impl ItemTextLookup {
    fn name_fields_for(&self, observation: &ItemObservation) -> BTreeMap<String, String> {
        if let Some(names) = self.exact_name_fields_for_encoded(observation.enc_name_hex.as_deref())
            && !names.is_empty()
        {
            return self.with_model_file_names(observation, names);
        }
        if let Some(names) =
            decode_encoded_name_fields(observation.enc_name_hex.as_deref(), &self.by_text_id)
            && !names.is_empty()
        {
            return self.with_model_file_names(observation, names);
        }
        if observation.name_id_is_exact
            && let Some(name_id) = observation.name_id
            && let Some(names) = self.by_text_id.get(&name_id).map(flat_runtime_name_fields)
            && !names.is_empty()
        {
            return self.with_model_file_names(observation, names);
        }

        if observation.enc_name_hex.is_some() {
            let names = self.model_file_name_fields_for(observation);
            return if names.is_empty() {
                self.generic_name_fields_for(observation)
            } else {
                names
            };
        }

        let names = self.fallback_name_fields_for(observation);
        if names.is_empty() {
            self.generic_name_fields_for(observation)
        } else {
            names
        }
    }

    fn exact_name_fields_for_encoded(
        &self,
        enc_name_hex: Option<&str>,
    ) -> Option<BTreeMap<String, String>> {
        let ids = asyncdecode_item_ids_from_hex(enc_name_hex?)?;
        if ids.is_empty() || ids.iter().any(|id| !self.exact_text_ids.contains(id)) {
            return None;
        }
        decode_name_fields_from_exact_ids(&ids, &self.by_text_id)
    }

    fn model_file_name_fields_for(
        &self,
        observation: &ItemObservation,
    ) -> BTreeMap<String, String> {
        self.by_model_file_id
            .get(&observation.model_file_id)
            .map(flat_runtime_name_fields)
            .unwrap_or_default()
    }

    fn with_model_file_names(
        &self,
        observation: &ItemObservation,
        mut names: BTreeMap<String, String>,
    ) -> BTreeMap<String, String> {
        for (key, value) in self.model_file_name_fields_for(observation) {
            names.entry(key).or_insert(value);
        }
        names
    }

    fn fallback_name_fields_for(&self, observation: &ItemObservation) -> BTreeMap<String, String> {
        let names = observation
            .name_id
            .and_then(|name_id| self.by_text_id.get(&name_id))
            .map(flat_runtime_name_fields)
            .unwrap_or_default();
        self.with_model_file_names(observation, names)
    }

    fn generic_name_fields_for(&self, observation: &ItemObservation) -> BTreeMap<String, String> {
        if observation.name_id != Some(GENERIC_ITEM_NAME_TEXT_ID) {
            return BTreeMap::new();
        }
        self.by_text_id
            .get(&GENERIC_ITEM_NAME_TEXT_ID)
            .map(|names| {
                names
                    .iter()
                    .filter_map(|(code, name)| {
                        let name = clean_display_text(name);
                        (!name.is_empty()).then(|| (format!("name_{code}"), name))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn validated_name_fields_for(
        &self,
        observation: &ItemObservation,
        official_name: &str,
    ) -> Option<BTreeMap<String, String>> {
        if let Some(ids) = observation
            .enc_name_hex
            .as_deref()
            .and_then(encoded_text_ids_from_hex)
            && let Some(names) = decode_name_fields_from_exact_ids(&ids, &self.by_text_id)
            && multilingual_names_match(&names, official_name)
        {
            return Some(self.with_model_file_names(observation, names));
        }
        if let Some(names) =
            decode_encoded_name_fields(observation.enc_name_hex.as_deref(), &self.by_text_id)
            && multilingual_names_match(&names, official_name)
        {
            return Some(self.with_model_file_names(observation, names));
        }

        let names = self.fallback_name_fields_for(observation);
        multilingual_names_match(&names, official_name).then_some(names)
    }
}

fn item_value<'a, V>(
    values: &'a BTreeMap<ItemKey, V>,
    item_id: u32,
    observation: &ItemObservation,
) -> Option<&'a V> {
    values
        .get(&(
            item_id,
            Some(observation.model_id),
            Some(observation.model_file_id),
        ))
        .or_else(|| values.get(&(item_id, None, None)))
}

fn unique_item_value<'a>(
    values: &'a BTreeMap<ItemKey, String>,
    item_ids: &BTreeSet<u32>,
    observation: &ItemObservation,
) -> Option<&'a String> {
    let mut matching = item_ids
        .iter()
        .filter_map(|item_id| item_value(values, *item_id, observation));
    let first = matching.next()?;
    let canonical = canonical_encoded_hex(first);
    matching
        .all(|value| canonical_encoded_hex(value).as_str() == canonical)
        .then_some(first)
}

fn merged_item_strings(
    values: &BTreeMap<ItemKey, BTreeMap<String, String>>,
    item_ids: &BTreeSet<u32>,
    observation: &ItemObservation,
) -> BTreeMap<String, String> {
    let mut merged = BTreeMap::new();
    let mut conflicts = BTreeSet::new();
    for strings in item_ids
        .iter()
        .filter_map(|item_id| item_value(values, *item_id, observation))
    {
        for (code, text) in strings {
            if conflicts.contains(code) {
                continue;
            }
            match merged.entry(code.clone()) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(text.clone());
                }
                std::collections::btree_map::Entry::Occupied(entry) if entry.get() == text => {}
                std::collections::btree_map::Entry::Occupied(entry) => {
                    entry.remove();
                    conflicts.insert(code.clone());
                }
            }
        }
    }
    merged
}

#[derive(Debug, Serialize)]
pub(super) struct ExtractedItem {
    model_id: u32,
    model_file_id: u32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    item_ids: Vec<u32>,
    #[serde(rename = "packet_name_id")]
    #[serde(skip_serializing_if = "Option::is_none")]
    name_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    item_type: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    extra_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    materials: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    interaction: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    price: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    runtime_description_available: Option<bool>,
    #[serde(flatten)]
    strings: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    observed_variants: Vec<ExtractedItemVariant>,
}

#[derive(Debug, Serialize)]
pub(super) struct ExtractedItemVariant {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    item_ids: Vec<u32>,
    #[serde(rename = "packet_name_id")]
    #[serde(skip_serializing_if = "Option::is_none")]
    name_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    item_type: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    extra_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    materials: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    interaction: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    price: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    runtime_description_available: Option<bool>,
    #[serde(flatten)]
    strings: BTreeMap<String, String>,
}

pub(super) fn aggregate_item(
    model_id: u32,
    model_file_id: u32,
    mut variants: Vec<ExtractedItemVariant>,
) -> ExtractedItem {
    if variants.len() == 1 {
        let variant = variants.pop().expect("single item variant");
        return ExtractedItem {
            model_id,
            model_file_id,
            item_ids: variant.item_ids,
            name_id: variant.name_id,
            item_type: variant.item_type,
            extra_id: variant.extra_id,
            materials: variant.materials,
            interaction: variant.interaction,
            price: variant.price,
            runtime_description_available: variant.runtime_description_available,
            strings: variant.strings,
            observed_variants: Vec::new(),
        };
    }

    let item_ids = variants
        .iter()
        .flat_map(|variant| variant.item_ids.iter().copied())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let best_index = variants
        .iter()
        .enumerate()
        .skip(1)
        .fold(0, |best, (index, variant)| {
            let best_variant = &variants[best];
            let score = (has_name_field(&variant.strings), variant.strings.len());
            let best_score = (
                has_name_field(&best_variant.strings),
                best_variant.strings.len(),
            );
            if score > best_score { index } else { best }
        });
    let runtime_description_available = variants
        .iter()
        .filter_map(|variant| variant.runtime_description_available)
        .reduce(|available, next| available || next);
    let best = &variants[best_index];
    ExtractedItem {
        model_id,
        model_file_id,
        item_ids,
        name_id: best.name_id,
        item_type: best.item_type,
        extra_id: best.extra_id,
        materials: best.materials,
        interaction: best.interaction,
        price: best.price,
        runtime_description_available,
        strings: best.strings.clone(),
        observed_variants: variants,
    }
}

#[cfg(test)]
pub(super) fn write_from_capture_for_test(
    packet_log_path: &Path,
    name_lookup: &ItemTextLookup,
    out_path: &Path,
) -> Result<()> {
    write_from_capture(packet_log_path, name_lookup, out_path, true)
}

#[cfg(test)]
pub(super) fn write_from_capture(
    packet_log_path: &Path,
    name_lookup: &ItemTextLookup,
    out_path: &Path,
    use_client_strings: bool,
) -> Result<()> {
    let capture = read_item_capture(packet_log_path, use_client_strings)?;
    write_catalog(capture, name_lookup, out_path)
}

pub(super) fn write_catalog(
    capture: ItemCapture,
    name_lookup: &ItemTextLookup,
    out_path: &Path,
) -> Result<()> {
    let ItemCapture {
        decoded_item_rows,
        observations,
        client_names_by_item,
        client_descriptions_by_item,
        client_name_rows,
        client_description_rows,
        runtime_string_rows,
        decoded_ids_by_encoded_hex,
        runtime_desc_hex_by_item,
        runtime_name_hex_by_item,
        runtime_desc_availability_by_item,
        ..
    } = capture;

    let mut items_by_model = BTreeMap::<(u32, u32), Vec<ExtractedItemVariant>>::new();
    for (observation, item_ids) in observations {
        let runtime_description_available = item_ids
            .iter()
            .filter_map(|item_id| {
                item_value(&runtime_desc_availability_by_item, *item_id, &observation)
            })
            .copied()
            .reduce(|available, next| available || next);
        let variant = ExtractedItemVariant {
            item_ids: item_ids.iter().copied().collect(),
            name_id: observation.name_id,
            item_type: observation.item_type,
            extra_id: observation.extra_id,
            materials: observation.materials,
            interaction: observation.interaction,
            price: observation.price,
            runtime_description_available,
            strings: {
                let runtime_name_hex =
                    unique_item_value(&runtime_name_hex_by_item, &item_ids, &observation);
                let local_names = name_lookup.with_model_file_names(
                    &observation,
                    observation
                        .enc_name_hex
                        .as_ref()
                        .and_then(|hex| decoded_ids_by_encoded_hex.get(&canonical_encoded_hex(hex)))
                        .and_then(|ids| {
                            decode_name_fields_from_exact_ids(ids, &name_lookup.by_text_id)
                        })
                        .or_else(|| {
                            runtime_name_hex
                                .and_then(|hex| {
                                    decoded_ids_by_encoded_hex.get(&canonical_encoded_hex(hex))
                                })
                                .and_then(|ids| {
                                    decode_name_fields_from_exact_ids(ids, &name_lookup.by_text_id)
                                })
                        })
                        .or_else(|| {
                            name_lookup
                                .exact_name_fields_for_encoded(runtime_name_hex.map(String::as_str))
                        })
                        .or_else(|| {
                            decode_encoded_name_fields(
                                runtime_name_hex.map(String::as_str),
                                &name_lookup.by_text_id,
                            )
                        })
                        .unwrap_or_else(|| name_lookup.name_fields_for(&observation)),
                );
                let client_names =
                    merged_item_strings(&client_names_by_item, &item_ids, &observation);
                let client_names =
                    client_runtime_name_fields(&client_names, &observation, name_lookup);
                let mut strings = if client_names.is_empty() {
                    local_names
                } else {
                    client_names
                };
                let desc_hex =
                    unique_item_value(&runtime_desc_hex_by_item, &item_ids, &observation)
                        .or(observation.desc_enc_hex.as_ref());
                let local_descriptions = desc_hex
                    .and_then(|hex| decoded_ids_by_encoded_hex.get(&canonical_encoded_hex(hex)))
                    .and_then(|ids| {
                        decode_description_fields_from_exact_ids(ids, &name_lookup.by_text_id)
                    })
                    .or_else(|| {
                        decode_encoded_description_fields(
                            desc_hex.map(String::as_str),
                            &name_lookup.by_text_id,
                        )
                    })
                    .unwrap_or_default();
                let client_descriptions =
                    merged_item_strings(&client_descriptions_by_item, &item_ids, &observation);
                let client_descriptions = client_runtime_description_fields(&client_descriptions);
                let descriptions = if client_descriptions.is_empty() {
                    local_descriptions
                } else {
                    client_descriptions
                };
                strings.extend(descriptions);
                strings
            },
        };

        items_by_model
            .entry((observation.model_id, observation.model_file_id))
            .or_default()
            .push(variant);
    }

    let items = items_by_model
        .into_iter()
        .map(|((model_id, model_file_id), variants)| {
            aggregate_item(model_id, model_file_id, variants)
        })
        .collect::<Vec<_>>();

    let named_items = items
        .iter()
        .filter(|item| has_name_field(&item.strings))
        .count();
    write_json(out_path, &items)?;
    println!(
        "exported {} runtime item rows from {} decoded packet rows, {} client decoded name rows, {} client decoded description rows, {} runtime string rows, and {} named item rows",
        items.len(),
        decoded_item_rows,
        client_name_rows,
        client_description_rows,
        runtime_string_rows,
        named_items,
    );
    Ok(())
}

pub(super) fn client_runtime_name_fields(
    names: &BTreeMap<String, String>,
    observation: &ItemObservation,
    name_lookup: &ItemTextLookup,
) -> BTreeMap<String, String> {
    let unique = valid_unique_texts(names);
    if unique.len() == 1 {
        let Some(name) = unique.into_iter().next() else {
            return BTreeMap::new();
        };
        if let Some(local_names) = name_lookup.validated_name_fields_for(observation, name) {
            return local_names;
        }
        return BTreeMap::from([("name".to_string(), name.clone())]);
    }
    flat_runtime_name_fields(names)
}

pub(super) fn client_runtime_description_fields(
    descriptions: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let unique = valid_unique_texts(descriptions);
    if unique.len() == 1 {
        let Some(description) = unique.into_iter().next() else {
            return BTreeMap::new();
        };
        return BTreeMap::from([("description".to_string(), description.clone())]);
    }
    flat_runtime_description_fields(descriptions)
}

pub(super) fn valid_unique_texts(strings: &BTreeMap<String, String>) -> BTreeSet<&String> {
    strings
        .values()
        .filter(|text| !is_invalid_label(text))
        .collect()
}

pub(super) fn has_name_field(strings: &BTreeMap<String, String>) -> bool {
    strings
        .keys()
        .any(|key| key == "name" || key.starts_with("name_"))
}

pub(super) fn multilingual_names_match(
    names: &BTreeMap<String, String>,
    official_name: &str,
) -> bool {
    localized_name_score(names) > 1
        && names
            .values()
            .any(|name| !is_invalid_label(name) && name == official_name)
}

pub(super) fn localized_name_score(names: &BTreeMap<String, String>) -> usize {
    valid_unique_texts(names).len()
}

pub(super) fn flat_runtime_description_fields(
    descriptions: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    flat_runtime_text_fields(descriptions, "description")
}
