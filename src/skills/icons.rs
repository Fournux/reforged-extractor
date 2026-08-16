use std::path::Path;

use anyhow::Context;

use crate::{atex, dat::DatArchive};

pub(super) fn export_skill_icon(
    archive: &mut DatArchive,
    texture_hash: u32,
    out_path: &Path,
) -> anyhow::Result<()> {
    let bytes = archive.read_file_id(texture_hash)?.with_context(|| {
        format!("skill icon file id {texture_hash} is missing from the DAT index")
    })?;
    atex::save_atex_as_png(&bytes, out_path)
}
