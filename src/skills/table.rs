use anyhow::{Context, bail};

use crate::pe::PeImage;

pub(super) const SKILL_RECORD_SIZE: usize = 164;

const SKILL_TABLE_RECORD_COUNT_OFFSET: usize = 0x2c;
const SKILL_TABLE_SCAN_ALIGNMENT: usize = std::mem::size_of::<u32>();
const MIN_SKILL_TABLE_RECORDS: usize = 512;
const MAX_SKILL_TABLE_RECORDS: usize = 10_000;
const MAX_PROBED_SKILL_ROWS: usize = 256;
const MIN_LIVE_SKILL_ROWS: usize = 32;
const NAME_DESCRIPTION_MATCH_SCORE: usize = 4;
const MAX_PLAUSIBLE_CAMPAIGN_CODE: u32 = 4;
const MAX_PLAUSIBLE_SKILL_TYPE_CODE: u32 = 29;
const MAX_PLAUSIBLE_PROFESSION_CODE: u8 = 10;
const MAX_PLAUSIBLE_EQUIP_TYPE_CODE: u8 = 3;

pub(super) struct SkillTable<'a> {
    bytes: &'a [u8],
}

pub(super) struct SkillRow<'a> {
    index: usize,
    bytes: &'a [u8],
}

impl<'a> SkillTable<'a> {
    pub(super) fn from_bytes(bytes: &'a [u8]) -> anyhow::Result<Self> {
        if !bytes.len().is_multiple_of(SKILL_RECORD_SIZE) {
            bail!("skill table length is not a multiple of {SKILL_RECORD_SIZE}");
        }
        Ok(Self { bytes })
    }

    pub(super) fn row(&self, index: usize) -> Option<SkillRow<'a>> {
        let start = index.checked_mul(SKILL_RECORD_SIZE)?;
        let end = start.checked_add(SKILL_RECORD_SIZE)?;
        self.bytes
            .get(start..end)
            .map(|bytes| SkillRow { index, bytes })
    }

    pub(super) fn rows(&self) -> impl Iterator<Item = SkillRow<'a>> + 'a {
        self.bytes
            .chunks_exact(SKILL_RECORD_SIZE)
            .enumerate()
            .map(|(index, bytes)| SkillRow { index, bytes })
    }
}

impl SkillRow<'_> {
    fn u16_at(&self, offset: usize) -> u16 {
        u16::from_le_bytes([self.bytes[offset], self.bytes[offset + 1]])
    }

    fn u32_at(&self, offset: usize) -> u32 {
        u32::from_le_bytes([
            self.bytes[offset],
            self.bytes[offset + 1],
            self.bytes[offset + 2],
            self.bytes[offset + 3],
        ])
    }

    fn f32_at(&self, offset: usize) -> f32 {
        f32::from_bits(self.u32_at(offset))
    }

    pub(super) fn index(&self) -> usize {
        self.index
    }

    pub(super) fn id(&self) -> u32 {
        self.u32_at(0x00)
    }

    pub(super) fn campaign_code(&self) -> u32 {
        self.u32_at(0x08)
    }

    pub(super) fn type_code(&self) -> u32 {
        self.u32_at(0x0c)
    }

    pub(super) fn flags(&self) -> u32 {
        self.u32_at(0x10)
    }

    pub(super) fn profession_code(&self) -> u8 {
        self.bytes[0x28]
    }

    pub(super) fn attribute_code(&self) -> u8 {
        self.bytes[0x29]
    }

    pub(super) fn title_track_code(&self) -> u16 {
        self.u16_at(0x2a)
    }

    pub(super) fn linked_skill_index(&self) -> usize {
        self.u32_at(0x2c) as usize
    }

    pub(super) fn target_code(&self) -> u8 {
        self.bytes[0x31]
    }

    pub(super) fn equip_type_code(&self) -> u8 {
        self.bytes[0x33]
    }

    pub(super) fn overcast_cost_raw(&self) -> u8 {
        self.bytes[0x34]
    }

    pub(super) fn energy_cost_encoded(&self) -> u8 {
        self.bytes[0x35]
    }

    pub(super) fn health_cost(&self) -> u8 {
        self.bytes[0x36]
    }

    pub(super) fn adrenaline_units(&self) -> u32 {
        self.u32_at(0x38)
    }

    pub(super) fn activation_seconds(&self) -> f32 {
        self.f32_at(0x3c)
    }

    pub(super) fn aftercast_seconds(&self) -> f32 {
        self.f32_at(0x40)
    }

    pub(super) fn duration_0_attribute(&self) -> u32 {
        self.u32_at(0x44)
    }

    pub(super) fn duration_15_attribute(&self) -> u32 {
        self.u32_at(0x48)
    }

    pub(super) fn recharge_seconds(&self) -> u32 {
        self.u32_at(0x4c)
    }

    pub(super) fn scale_0(&self) -> u32 {
        self.u32_at(0x5c)
    }

    pub(super) fn scale_15(&self) -> u32 {
        self.u32_at(0x60)
    }

    pub(super) fn bonus_scale_0(&self) -> u32 {
        self.u32_at(0x64)
    }

    pub(super) fn bonus_scale_15(&self) -> u32 {
        self.u32_at(0x68)
    }

    pub(super) fn aoe_range(&self) -> f32 {
        self.f32_at(0x6c)
    }

    pub(super) fn constant_effect(&self) -> f32 {
        self.f32_at(0x70)
    }

    pub(super) fn icon_texture_hash(&self) -> u32 {
        self.u32_at(0x8c)
    }

    pub(super) fn icon_hd_texture_hash(&self) -> u32 {
        self.u32_at(0x90)
    }

    pub(super) fn name_string_id(&self) -> u32 {
        self.u32_at(0x98)
    }

    pub(super) fn description_string_id(&self) -> u32 {
        self.u32_at(0xa0)
    }
}

struct SkillTableDetection {
    file_offset: usize,
    record_count: usize,
    score: usize,
}

fn read_u32_at(data: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(std::mem::size_of::<u32>())?;
    let bytes = data.get(offset..end)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn skill_table_probe_score(data: &[u8], offset: usize, record_count: usize) -> Option<usize> {
    let table_len = record_count.checked_mul(SKILL_RECORD_SIZE)?;
    let table_end = offset.checked_add(table_len)?;
    let table = SkillTable::from_bytes(data.get(offset..table_end)?).ok()?;

    let header = table.row(0)?;
    if header.id() != 0 || header.linked_skill_index() != record_count {
        return None;
    }

    let first_live = table.row(1)?;
    if first_live.id() != 1 {
        return None;
    }
    let first_name = first_live.name_string_id();
    let first_desc = first_live.description_string_id();
    if first_name == 0 || first_name.checked_add(1) != Some(first_desc) {
        return None;
    }

    let mut score = 0_usize;
    let mut live_like = 0_usize;
    for row in table.rows().skip(1).take(MAX_PROBED_SKILL_ROWS - 1) {
        let name_id = row.name_string_id();
        if name_id == 0 {
            continue;
        }
        if name_id.checked_add(1) == Some(row.description_string_id()) {
            score += NAME_DESCRIPTION_MATCH_SCORE;
            live_like += 1;
        }
        if row.campaign_code() <= MAX_PLAUSIBLE_CAMPAIGN_CODE {
            score += 1;
        }
        if row.type_code() <= MAX_PLAUSIBLE_SKILL_TYPE_CODE {
            score += 1;
        }
        if row.profession_code() <= MAX_PLAUSIBLE_PROFESSION_CODE {
            score += 1;
        }
        if row.equip_type_code() <= MAX_PLAUSIBLE_EQUIP_TYPE_CODE {
            score += 1;
        }
    }

    if live_like < MIN_LIVE_SKILL_ROWS {
        return None;
    }
    Some(score)
}

pub(super) fn locate_skill_table<'data>(pe: &PeImage<'data>) -> anyhow::Result<SkillTable<'data>> {
    let pe_data = pe.data();
    let mut best: Option<SkillTableDetection> = None;
    for section in pe.sections() {
        let raw_range = section.raw_range();
        let raw_start = raw_range.start;
        let raw_end = raw_range.end;
        let Some(minimum_table_end) = raw_start.checked_add(SKILL_RECORD_SIZE * 2) else {
            continue;
        };
        if raw_end <= minimum_table_end {
            continue;
        }

        let mut offset = raw_start;
        while offset
            .checked_add(SKILL_RECORD_SIZE * 2)
            .is_some_and(|minimum_end| minimum_end <= raw_end)
        {
            let Some(count_offset) = offset.checked_add(SKILL_TABLE_RECORD_COUNT_OFFSET) else {
                break;
            };
            let Some(record_count) = read_u32_at(pe_data, count_offset).map(|value| value as usize)
            else {
                break;
            };
            if let Some(table_end) = record_count
                .checked_mul(SKILL_RECORD_SIZE)
                .and_then(|len| offset.checked_add(len))
                && (MIN_SKILL_TABLE_RECORDS..=MAX_SKILL_TABLE_RECORDS).contains(&record_count)
                && table_end <= raw_end
                && let Some(score) = skill_table_probe_score(pe_data, offset, record_count)
            {
                let candidate = SkillTableDetection {
                    file_offset: offset,
                    record_count,
                    score,
                };
                if best
                    .as_ref()
                    .is_none_or(|current| candidate.score > current.score)
                {
                    best = Some(candidate);
                }
            }

            let Some(next_offset) = offset.checked_add(SKILL_TABLE_SCAN_ALIGNMENT) else {
                break;
            };
            offset = next_offset;
        }
    }

    let detection =
        best.with_context(|| "failed to locate s_skill table structurally in client PE")?;
    let table_len = detection
        .record_count
        .checked_mul(SKILL_RECORD_SIZE)
        .context("skill table byte length overflow")?;
    let table_end = detection
        .file_offset
        .checked_add(table_len)
        .context("skill table end offset overflow")?;
    let bytes = pe_data
        .get(detection.file_offset..table_end)
        .context("skill table exceeds PE data")?;
    SkillTable::from_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    #[test]
    fn rejects_partial_skill_row() {
        let bytes = vec![0; SKILL_RECORD_SIZE + 1];
        assert!(SkillTable::from_bytes(&bytes).is_err());
    }

    #[test]
    fn scores_structural_candidate_without_wrapping_string_ids() {
        let offset = SKILL_TABLE_SCAN_ALIGNMENT;
        let record_count = MIN_SKILL_TABLE_RECORDS;
        let mut data = vec![0; offset + record_count * SKILL_RECORD_SIZE];
        let table = &mut data[offset..];
        set_u32(table, SKILL_TABLE_RECORD_COUNT_OFFSET, record_count as u32);

        for index in 1..=MIN_LIVE_SKILL_ROWS + 1 {
            let row_start = index * SKILL_RECORD_SIZE;
            let row = &mut table[row_start..row_start + SKILL_RECORD_SIZE];
            set_u32(row, 0, index as u32);
            let name_id = 1_000 + index as u32 * 2;
            set_u32(row, 0x98, name_id);
            set_u32(row, 0xa0, name_id + 1);
        }

        assert!(skill_table_probe_score(&data, offset, record_count).is_some());

        let second_row = offset + SKILL_RECORD_SIZE * 2;
        set_u32(&mut data, second_row + 0x98, u32::MAX);
        set_u32(&mut data, second_row + 0xa0, 0);
        assert!(skill_table_probe_score(&data, offset, record_count).is_some());

        let first_row = offset + SKILL_RECORD_SIZE;
        set_u32(&mut data, first_row + 0x98, u32::MAX);
        set_u32(&mut data, first_row + 0xa0, 0);
        assert!(skill_table_probe_score(&data, offset, record_count).is_none());
    }
}
