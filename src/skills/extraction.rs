use super::{
    EXPECTED_SKILL_DISTRIBUTION, ExtractedSkill, OutputCampaignStats, OutputCounts, OutputManifest,
    SKILL_FLAG_ELITE, SKILL_FLAG_HALF_RANGE, SKILL_FLAG_NON_STACKING, SKILL_FLAG_NOT_PLAYABLE,
    SKILL_FLAG_PVE, SKILL_FLAG_PVP, SKILL_FLAG_STACKING, SKILL_FLAG_TOUCH_RANGE,
    SKILL_OUTPUT_SCHEMA_VERSION, SkillCosts, SkillFlags, SkillScaling, SkillTiming,
    adrenaline_strikes, attribute_name, campaign_name, decoded_energy_cost,
    icons::export_skill_icon,
    overcast_cost, profession_name, skill_type_name,
    table::{SkillTable, locate_skill_table},
    validate_skill_distribution,
};

use anyhow::{Context, bail};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use crate::{
    dat::DatArchive,
    io_util::write_json,
    pe::PeImage,
    text::{catalog::LocalizedTextReader, clean_display_text},
};

const UNVALIDATED_SKILL_ID: usize = 3442;
const PVP_VARIANT_EQUIP_USE_FAMILY: u8 = 0;
const BASE_SKILL_EQUIP_USE_FAMILY: u8 = 1;

fn select_skill_indices(skill_table: &SkillTable<'_>) -> anyhow::Result<BTreeSet<usize>> {
    let mut selected_indices = BTreeSet::new();
    for row in skill_table.rows() {
        let index = row.index();
        if row.id() != index as u32 {
            bail!("skill table row {index} has a mismatched skill id");
        }

        if index == UNVALIDATED_SKILL_ID {
            continue;
        }

        let flags = row.flags();
        let standard =
            flags & SKILL_FLAG_PVP == 0 && row.equip_type_code() == BASE_SKILL_EQUIP_USE_FAMILY;
        let base_index = row.linked_skill_index();
        let current_pvp_variant = flags & SKILL_FLAG_PVP != 0
            && row.equip_type_code() == PVP_VARIANT_EQUIP_USE_FAMILY
            && skill_table.row(base_index).is_some_and(|base_row| {
                base_row.flags() & SKILL_FLAG_PVP == 0
                    && base_row.equip_type_code() == BASE_SKILL_EQUIP_USE_FAMILY
                    && base_row.linked_skill_index() == index
            });
        if standard || current_pvp_variant {
            selected_indices.insert(index);
        }
    }

    Ok(selected_indices)
}

pub(crate) fn extract_skills_to_model_file_dirs(
    gw_dat_path: &Path,
    out_path: &Path,
    model_file_dir: &Path,
    model_file_hd_dir: &Path,
) -> anyhow::Result<()> {
    let mut archive = DatArchive::open(gw_dat_path)?;
    let pe_data = archive.client_pe_data()?;
    let pe = PeImage::parse(&pe_data)?;

    // Extract skill metadata records from the PE skill table
    let skill_table = locate_skill_table(&pe)?;

    let compact_seeds = BTreeMap::new();
    let decoded_records = BTreeMap::new();
    let mut text_reader =
        LocalizedTextReader::new(&mut archive, &pe, &compact_seeds, &decoded_records)?;

    let selected_indices = select_skill_indices(&skill_table)?;

    let mut extracted_skills = Vec::new();
    let mut icon_jobs = Vec::new();

    for index in selected_indices {
        let row = skill_table
            .row(index)
            .with_context(|| format!("selected skill {index} row is missing"))?;
        let name_string_id = row.name_string_id();
        let description_string_id = row.description_string_id();

        let name = text_reader
            .text(name_string_id)?
            .into_iter()
            .filter_map(|(code, text)| {
                let text = clean_display_text(&text);
                (!text.is_empty()).then_some((code, text))
            })
            .collect();
        let description = text_reader
            .text(description_string_id)?
            .into_iter()
            .filter_map(|(code, text)| {
                let text = clean_display_text(&text);
                (!text.is_empty()).then_some((code, text))
            })
            .collect();

        let icon_texture_hash = row.icon_texture_hash();

        let campaign_code = row.campaign_code();
        let campaign = campaign_name(campaign_code);
        let title_track_code = row.title_track_code();
        let effective_campaign = match title_track_code {
            5 | 6 => "factions",
            _ => campaign,
        };

        let flags_val = row.flags();
        let touch_range = (flags_val & SKILL_FLAG_TOUCH_RANGE) != 0;
        let elite = (flags_val & SKILL_FLAG_ELITE) != 0;
        let half_range = (flags_val & SKILL_FLAG_HALF_RANGE) != 0;
        let stacking = (flags_val & SKILL_FLAG_STACKING) != 0;
        let non_stacking = (flags_val & SKILL_FLAG_NON_STACKING) != 0;
        let pvp = (flags_val & SKILL_FLAG_PVP) != 0;
        let pve = (flags_val & SKILL_FLAG_PVE) != 0;
        let playable = (flags_val & SKILL_FLAG_NOT_PLAYABLE) == 0;

        let profession_code = row.profession_code();
        let type_code = row.type_code();
        let energy_cost_encoded = row.energy_cost_encoded();
        let adrenaline_units = row.adrenaline_units();
        let skill_equip_type_code = row.equip_type_code();
        let duration_0_attribute = row.duration_0_attribute();
        let duration_15_attribute = row.duration_15_attribute();
        let scale_0 = row.scale_0();
        let scale_15 = row.scale_15();
        let bonus_scale_0 = row.bonus_scale_0();
        let bonus_scale_15 = row.bonus_scale_15();

        let icon_hd_texture_hash = row.icon_hd_texture_hash();

        icon_jobs.push((index, icon_texture_hash, icon_hd_texture_hash));

        extracted_skills.push(ExtractedSkill {
            id: row.id(),
            name,
            description,
            campaign: effective_campaign.to_string(),
            profession: profession_name(profession_code).to_string(),
            profession_code,
            attribute_code: row.attribute_code(),
            attribute: attribute_name(row.attribute_code(), profession_code).to_string(),
            skill_type: skill_type_name(type_code).to_string(),
            type_code,
            elite,
            costs: SkillCosts {
                energy: decoded_energy_cost(energy_cost_encoded),
                energy_encoded: energy_cost_encoded,
                health: row.health_cost(),
                adrenaline: adrenaline_strikes(adrenaline_units),
                adrenaline_units,
                overcast: overcast_cost(flags_val, row.overcast_cost_raw()),
            },
            timing: SkillTiming {
                activation_seconds: row.activation_seconds(),
                aftercast_seconds: row.aftercast_seconds(),
                recharge_seconds: row.recharge_seconds(),
                duration_0_attribute,
                duration_15_attribute,
            },
            scaling: SkillScaling {
                scale_0,
                scale_15,
                bonus_scale_0,
                bonus_scale_15,
            },
            target_code: row.target_code(),
            aoe_range: row.aoe_range(),
            constant_effect: row.constant_effect(),
            skill_equip_type_code,
            flags: SkillFlags {
                touch_range,
                elite,
                half_range,
                stacking,
                non_stacking,
                pvp,
                pve,
                playable,
            },
        });
    }

    let mut campaigns_stats = BTreeMap::new();
    for (campaign, _, _) in EXPECTED_SKILL_DISTRIBUTION {
        let (non_elite, elite) = extracted_skills
            .iter()
            .filter(|s| s.campaign.as_str() == campaign)
            .fold(
                (0, 0),
                |(ne, el), s| if s.elite { (ne, el + 1) } else { (ne + 1, el) },
            );
        campaigns_stats.insert(
            campaign.to_string(),
            OutputCampaignStats {
                non_elite,
                elite,
                total: non_elite + elite,
            },
        );
    }
    validate_skill_distribution(&campaigns_stats, extracted_skills.len())?;

    drop(text_reader);
    fs::create_dir_all(model_file_dir)
        .with_context(|| format!("creating {}", model_file_dir.display()))?;
    fs::create_dir_all(model_file_hd_dir)
        .with_context(|| format!("creating {}", model_file_hd_dir.display()))?;
    for (index, icon_texture_hash, icon_hd_texture_hash) in icon_jobs {
        let icon_path = model_file_dir.join(format!("{index}.png"));
        export_skill_icon(&mut archive, icon_texture_hash, &icon_path)
            .with_context(|| format!("exporting skill {index} icon"))?;
        if icon_hd_texture_hash != 0 {
            let icon_path = model_file_hd_dir.join(format!("{index}.png"));
            export_skill_icon(&mut archive, icon_hd_texture_hash, &icon_path)
                .with_context(|| format!("exporting skill {index} HD icon"))?;
        }
    }

    let final_output = OutputManifest {
        schema_version: SKILL_OUTPUT_SCHEMA_VERSION,
        counts: OutputCounts {
            skills: extracted_skills.len(),
            campaigns: campaigns_stats,
        },
        skills: extracted_skills,
    };
    write_json(out_path, &final_output)
}

#[cfg(test)]
mod tests {
    use super::super::table::SKILL_RECORD_SIZE;
    use super::*;

    fn set_u32(row: &mut [u8], offset: usize, value: u32) {
        row[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    #[test]
    fn selects_standard_skills_and_their_current_pvp_variants() {
        let mut table = vec![0; SKILL_RECORD_SIZE * 6];
        for (index, row) in table.chunks_exact_mut(SKILL_RECORD_SIZE).enumerate() {
            set_u32(row, 0, index as u32);
        }

        let base = &mut table[SKILL_RECORD_SIZE..SKILL_RECORD_SIZE * 2];
        base[0x33] = 1;
        set_u32(base, 0x2c, 3);

        let hidden = &mut table[SKILL_RECORD_SIZE * 2..SKILL_RECORD_SIZE * 3];
        hidden[0x33] = 2;

        let pvp = &mut table[SKILL_RECORD_SIZE * 3..SKILL_RECORD_SIZE * 4];
        set_u32(pvp, 0x10, SKILL_FLAG_PVP);
        set_u32(pvp, 0x2c, 1);

        let stale_pvp = &mut table[SKILL_RECORD_SIZE * 4..SKILL_RECORD_SIZE * 5];
        set_u32(stale_pvp, 0x10, SKILL_FLAG_PVP);

        let special = &mut table[SKILL_RECORD_SIZE * 5..];
        set_u32(special, 0x10, SKILL_FLAG_NOT_PLAYABLE);
        special[0x33] = 1;

        let skill_table = SkillTable::from_bytes(&table).unwrap();
        assert_eq!(
            select_skill_indices(&skill_table)
                .unwrap()
                .into_iter()
                .collect::<Vec<_>>(),
            vec![1, 3, 5]
        );
    }

    #[test]
    fn ignores_the_skill_added_after_the_validated_corpus() {
        let mut table = vec![0; SKILL_RECORD_SIZE * (UNVALIDATED_SKILL_ID + 1)];
        for (index, row) in table.chunks_exact_mut(SKILL_RECORD_SIZE).enumerate() {
            set_u32(row, 0, index as u32);
        }
        table[UNVALIDATED_SKILL_ID * SKILL_RECORD_SIZE + 0x33] = 1;

        let skill_table = SkillTable::from_bytes(&table).unwrap();
        assert!(select_skill_indices(&skill_table).unwrap().is_empty());
    }
}
