use std::collections::{BTreeMap, BTreeSet};

use crate::text::{
    apply_encoded_template, encoded_values_from_hex, encoded_values_from_words,
    encoded_words_from_hex, text_references,
};

pub(super) const GENERIC_ITEM_NAME_TEXT_ID: u32 = 8326;

// Clean item names, plurals, brackets, etc.
pub(super) fn clean_item_name(text: &str) -> String {
    let mut text = text.replace("[lbracket]", "[").replace("[rbracket]", "]");

    // Replace bracket tags: [proper], [F], [M], [N], [PF], [PM], [U]
    let bracket_tags = ["proper", "F", "M", "N", "PF", "PM", "U"];
    for tag in &bracket_tags {
        text = text.replace(&format!("[{tag}]"), "");
    }

    // Remove plural/gender selector tags exactly like the Python validator:
    // [s], [pl:"..."], [f:"..."], and [m:"..."] are markup, not display text.
    while let Some(start) = text.find('[') {
        if let Some(end) = text[start..].find(']') {
            text.replace_range(start..=start + end, "");
        } else {
            break;
        }
    }

    // Strip HTML/XML tags
    while let Some(start) = text.find('<') {
        if let Some(end) = text[start..].find('>') {
            text.replace_range(start..=start + end, "");
        } else {
            break;
        }
    }

    // Collapse whitespace and trim
    let mut result = String::new();
    let mut in_space = false;
    for c in text.chars() {
        if c.is_whitespace() {
            if !in_space {
                result.push(' ');
                in_space = true;
            }
        } else {
            result.push(c);
            in_space = false;
        }
    }
    result
        .trim_matches(|c: char| c.is_whitespace() || c == '\0')
        .to_string()
}

pub(super) fn looks_like_item_name(raw_text: &str) -> bool {
    if raw_text.is_empty() || raw_text == "[null]" || raw_text == "..." || raw_text == "Unknown" {
        return false;
    }
    let lower_raw = raw_text.to_lowercase();
    let bad_substrings = [
        "%str",
        "%num",
        "guildwars.com",
        "http://",
        "https://",
        "<img",
        "\n",
    ];
    if bad_substrings.iter().any(|bad| lower_raw.contains(bad)) {
        return false;
    }

    let text = clean_item_name(raw_text);
    if text.is_empty() || text.len() < 2 || text.len() > 80 {
        return false;
    }
    if is_invalid_label(&text) {
        return false;
    }
    if !text.chars().any(|c| c.is_ascii_alphabetic()) {
        return false;
    }

    let has_digit = text.chars().any(|c| c.is_ascii_digit());
    if has_digit {
        let words: BTreeSet<&str> = text.split(|c: char| !c.is_alphanumeric()).collect();
        let allowed = ["1st", "2nd", "3rd", "Zaishen", "PvP"];
        if !allowed.iter().any(|&w| words.contains(w)) {
            return false;
        }
    }

    let lower = text.to_lowercase();
    let invalid_prefixes = [
        "you ",
        "your ",
        "target ",
        "for ",
        "while ",
        "if ",
        "when ",
        "this ",
        "use ",
        "speak",
        "stores",
        "applying",
        "enter",
        "choose",
        "double-click",
        "speak with",
        "stores ",
        "applying ",
        "enter ",
        "choose ",
    ];
    if invalid_prefixes
        .iter()
        .any(|prefix| lower.starts_with(prefix))
    {
        return false;
    }
    if text.contains('?') || text.contains(';') || text.ends_with('.') {
        return false;
    }
    if text.ends_with('!') && text.split_whitespace().count() > 4 {
        return false;
    }
    if text.split_whitespace().count() > 7 {
        return false;
    }

    true
}

pub(super) fn is_invalid_label(name: &str) -> bool {
    let invalid_labels = [
        "Default",
        "Accept",
        "Reject",
        "Fail",
        "Fault",
        "Travel",
        "America",
        "Asia",
        "Europe",
        "International",
        "More Options...",
        "Active District",
        "Districts...",
        "Unknown",
        "Inconnu",
        "Unbekannt",
        "Desconocido",
        "Sconosciuto",
        "Nieznany",
        "未知的",
        "알 수 없음",
        "不明",
    ];
    invalid_labels.contains(&name)
}

#[cfg(test)]
pub(crate) fn encoded_values_for_test(hex: &str) -> Vec<u64> {
    encoded_values_from_hex(hex)
        .unwrap_or_default()
        .into_iter()
        .map(|(value, _, _)| value)
        .collect()
}

#[cfg(test)]
pub(crate) fn encoded_value_spans_for_test(hex: &str) -> (Vec<u16>, Vec<(u64, usize, usize)>) {
    let words = encoded_words_from_hex(hex).unwrap_or_default();
    let spans = encoded_values_from_words(&words).unwrap_or_default();
    (words, spans)
}

#[cfg(test)]
pub(crate) fn asyncdecode_item_ids_for_test(hex: &str) -> Vec<u32> {
    asyncdecode_item_ids_from_hex(hex).unwrap_or_default()
}

// ponytail: observed item AsyncDecode subset; add the full client opcode VM only when new captures break it.
pub(super) fn decode_encoded_name_fields(
    enc_name_hex: Option<&str>,
    by_text_id: &BTreeMap<u32, BTreeMap<String, String>>,
) -> Option<BTreeMap<String, String>> {
    let hex = enc_name_hex?;
    asyncdecode_item_ids_from_hex(hex)
        .and_then(|ids| decode_text_fields_from_ids(&ids, by_text_id, "name"))
        .or_else(|| {
            let ids = encoded_name_ids_from_hex(hex)?;
            decode_text_fields_from_ids(&ids, by_text_id, "name")
        })
}

pub(super) fn decode_encoded_description_fields(
    desc_enc_hex: Option<&str>,
    by_text_id: &BTreeMap<u32, BTreeMap<String, String>>,
) -> Option<BTreeMap<String, String>> {
    let hex = desc_enc_hex?;
    asyncdecode_item_ids_from_hex(hex)
        .and_then(|ids| decode_text_fields_from_exact_ids(&ids, by_text_id, "description"))
        .or_else(|| {
            let ids = encoded_name_ids_from_hex(hex)?;
            decode_text_fields_from_exact_ids(&ids, by_text_id, "description")
        })
}

pub(super) fn decode_name_fields_from_exact_ids(
    ids: &[u32],
    by_text_id: &BTreeMap<u32, BTreeMap<String, String>>,
) -> Option<BTreeMap<String, String>> {
    decode_text_fields_from_exact_ids(ids, by_text_id, "name")
}

pub(super) fn decode_description_fields_from_exact_ids(
    ids: &[u32],
    by_text_id: &BTreeMap<u32, BTreeMap<String, String>>,
) -> Option<BTreeMap<String, String>> {
    decode_text_fields_from_exact_ids(ids, by_text_id, "description")
}

pub(super) fn decode_text_fields_from_exact_ids(
    ids: &[u32],
    by_text_id: &BTreeMap<u32, BTreeMap<String, String>>,
    prefix: &str,
) -> Option<BTreeMap<String, String>> {
    if let [id] = ids {
        return by_text_id
            .get(id)
            .map(|names| flat_runtime_text_fields(names, prefix))
            .filter(|fields| !fields.is_empty());
    }
    decode_text_fields_from_ids(ids, by_text_id, prefix)
}

pub(super) fn decode_text_fields_from_ids(
    ids: &[u32],
    by_text_id: &BTreeMap<u32, BTreeMap<String, String>>,
    prefix: &str,
) -> Option<BTreeMap<String, String>> {
    let (template_id, arg_ids) = ids.split_first()?;
    if arg_ids.is_empty() {
        return None;
    }
    let template_names = by_text_id.get(template_id)?;
    let mut fields = BTreeMap::new();
    for (code, template) in template_names {
        let args = arg_ids
            .iter()
            .filter_map(|id| {
                by_text_id
                    .get(id)
                    .and_then(|names| names.get(code).or_else(|| names.get("en")))
                    .cloned()
            })
            .collect::<Vec<_>>();
        let text = clean_item_name(&apply_encoded_template(template, &args));
        if text.is_empty()
            || text == "[null]"
            || encoded_name_has_unresolved_placeholder(&text)
            || is_invalid_label(&text)
        {
            continue;
        }
        let text = clean_item_name(text.trim_end_matches('%'));
        if !text.is_empty() && !is_invalid_label(&text) {
            fields.insert(format!("{prefix}_{code}"), text);
        }
    }
    (!fields.is_empty()).then_some(fields)
}

const ASYNCDECODE_ITEM_CONTROL_WORDS: &[u16] = &[
    0x0a30, 0x0a31, 0x0a33, 0x0a34, 0x0a35, 0x0a3a, 0x0a3b, 0x0a3c, 0x0a3d, 0x0a3e, 0x0a3f, 0x0a40,
    0x0a42, 0x0a43, 0x0a7e, 0x0a80, 0x0a81, 0x0a84, 0x0a85, 0x0a86, 0x0a87, 0x0a88, 0x0a89, 0x0a8a,
    0x0a8b, 0x0aa4, 0x0aa7, 0x0aa8, 0x0aa9, 0x0aac, 0x0aaf, 0x0abb, 0x0abc,
];

const ASYNCDECODE_ITEM_CONTROL_IDS: &[u64] = &[37_404, 56_261, 69_415];

pub(super) fn encoded_name_ids_from_hex(hex: &str) -> Option<Vec<u32>> {
    Some(
        encoded_values_from_hex(hex)?
            .into_iter()
            .filter_map(|(value, _, _)| u32::try_from(value).ok())
            .collect(),
    )
}

pub(super) fn asyncdecode_item_ids_from_hex(hex: &str) -> Option<Vec<u32>> {
    let words = encoded_words_from_hex(hex)?;
    let values = encoded_values_from_words(&words)?;
    Some(
        values
            .into_iter()
            .filter(|(value, start, end)| {
                should_emit_asyncdecode_item_id(&words, *value, *start, *end)
            })
            .filter_map(|(value, _, _)| u32::try_from(value).ok())
            .collect(),
    )
}

pub(super) fn should_emit_asyncdecode_item_id(
    words: &[u16],
    value: u64,
    start: usize,
    end: usize,
) -> bool {
    if value == 2 {
        return start > 0
            && end < words.len()
            && words[start - 1] == 0x0002
            && words[end] == 0x0002;
    }
    if value > u64::from(u32::MAX) {
        return false;
    }
    if end == start + 1 {
        let word = words[start];
        return word >= 0x08d4 && !ASYNCDECODE_ITEM_CONTROL_WORDS.contains(&word);
    }
    !ASYNCDECODE_ITEM_CONTROL_IDS.contains(&value)
}

pub(super) fn encoded_name_seeds_from_hex(hex: &str) -> Option<BTreeMap<u32, u64>> {
    Some(
        text_references(&encoded_words_from_hex(hex)?)
            .into_iter()
            .filter(|reference| reference.seed > u64::from(u32::MAX))
            .map(|reference| (reference.id, reference.seed))
            .collect(),
    )
}

pub(super) fn encoded_name_has_unresolved_placeholder(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    ["%str", "%num", "%s", "%d"]
        .iter()
        .any(|placeholder| lower.contains(placeholder))
}

pub(super) fn flat_runtime_text_fields(
    strings: &BTreeMap<String, String>,
    prefix: &str,
) -> BTreeMap<String, String> {
    strings
        .iter()
        .filter(|(_, text)| !is_invalid_label(text))
        .map(|(code, text)| (format!("{prefix}_{code}"), text.clone()))
        .collect()
}
