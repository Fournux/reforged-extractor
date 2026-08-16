use anyhow::{Context, bail};

use crate::pe::PeImage;

pub(super) const SKILL_RECORD_SIZE: usize = 164;

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

pub(super) struct SkillTableDetection {
    pub(super) file_offset: usize,
    pub(super) record_count: usize,
    score: usize,
}

fn read_u32_at(data: &[u8], offset: usize) -> Option<u32> {
    let bytes = data.get(offset..offset + 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn skill_table_probe_score(data: &[u8], offset: usize, record_count: usize) -> Option<usize> {
    if offset + record_count.checked_mul(SKILL_RECORD_SIZE)? > data.len() {
        return None;
    }
    if read_u32_at(data, offset)? != 0 {
        return None;
    }
    let declared_count = read_u32_at(data, offset + 0x2c)? as usize;
    if declared_count != record_count {
        return None;
    }

    let first_live = offset + SKILL_RECORD_SIZE;
    if read_u32_at(data, first_live)? != 1 {
        return None;
    }
    let first_name = read_u32_at(data, first_live + 0x98)?;
    let first_desc = read_u32_at(data, first_live + 0xa0)?;
    if first_name == 0 || first_desc != first_name + 1 {
        return None;
    }

    let probe_count = record_count.min(256);
    let mut score = 0_usize;
    let mut live_like = 0_usize;
    for index in 1..probe_count {
        let rec = offset + index * SKILL_RECORD_SIZE;
        let name_id = read_u32_at(data, rec + 0x98)?;
        let desc_id = read_u32_at(data, rec + 0xa0)?;
        if name_id == 0 {
            continue;
        }
        let campaign = read_u32_at(data, rec + 0x08)?;
        let type_code = read_u32_at(data, rec + 0x0c)?;
        let profession = *data.get(rec + 0x28)?;
        let equip_type = *data.get(rec + 0x33)?;

        if desc_id == name_id + 1 {
            score += 4;
            live_like += 1;
        }
        if campaign <= 4 {
            score += 1;
        }
        if type_code <= 29 {
            score += 1;
        }
        if profession <= 10 {
            score += 1;
        }
        if equip_type <= 3 {
            score += 1;
        }
    }

    if live_like < 32 {
        return None;
    }
    Some(score)
}

pub(super) fn locate_skill_table(
    pe_data: &[u8],
    pe: &PeImage,
) -> anyhow::Result<SkillTableDetection> {
    let mut best: Option<SkillTableDetection> = None;
    for section in pe.sections() {
        let raw_start = section.raw_pointer as usize;
        let raw_end = raw_start
            .saturating_add(section.raw_size as usize)
            .min(pe_data.len());
        if raw_end <= raw_start + SKILL_RECORD_SIZE * 2 {
            continue;
        }
        let mut offset = raw_start;
        while offset + SKILL_RECORD_SIZE * 2 <= raw_end {
            let Some(record_count) =
                read_u32_at(pe_data, offset + 0x2c).map(|value| value as usize)
            else {
                break;
            };
            let Some(table_end) = record_count
                .checked_mul(SKILL_RECORD_SIZE)
                .and_then(|len| offset.checked_add(len))
            else {
                offset += 4;
                continue;
            };
            if (512..=10000).contains(&record_count)
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
            offset += 4;
        }
    }

    best.with_context(|| "failed to locate s_skill table structurally in client PE")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_partial_skill_row() {
        let bytes = vec![0; SKILL_RECORD_SIZE + 1];
        assert!(SkillTable::from_bytes(&bytes).is_err());
    }
}
