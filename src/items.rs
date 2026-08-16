mod archive;
mod capture;
mod catalog;
mod icons;
mod text;

use anyhow::Context;
use std::path::Path;

#[cfg(test)]
mod tests;

pub(crate) use icons::export_model_file_icons;

pub(crate) fn extract_catalog(
    snapshot: &Path,
    capture_path: &Path,
    out_path: &Path,
    use_client_strings: bool,
) -> anyhow::Result<()> {
    let text_inputs = capture::packet_log_text_inputs(capture_path, use_client_strings)
        .with_context(|| format!("reading item text inputs from {}", capture_path.display()))?;
    let text_lookup = archive::resolve_item_text_lookup(
        snapshot,
        &text_inputs.name_ids,
        &text_inputs.compact_seeds,
        &text_inputs.decoded_records,
    )
    .with_context(|| format!("resolving item names from {}", snapshot.display()))?;
    catalog::write_from_capture(capture_path, &text_lookup, out_path, use_client_strings)
        .with_context(|| format!("extracting runtime items from {}", capture_path.display()))
}

#[cfg(test)]
pub(crate) use icons::{
    export_model_file_icon_payload_for_test, find_inline_atex_payload,
    model_file_icon_candidate_for_test,
};
