use std::{mem::size_of, ops::Range};

use anyhow::{Context, bail};

use crate::{
    file_ref::decode_file_reference,
    io_util::{read_u16, read_u32},
};
const DOS_SIGNATURE: &[u8; 2] = b"MZ";
const DOS_E_LFANEW_OFFSET: usize = 0x3c;
const U32_SIZE: usize = size_of::<u32>();
const DOS_HEADER_MIN_SIZE: usize = DOS_E_LFANEW_OFFSET + U32_SIZE;
const PE_SIGNATURE: &[u8; 4] = b"PE\0\0";
const COFF_HEADER_SIZE: usize = 20;
const COFF_SECTION_COUNT_OFFSET: usize = 2;
const COFF_OPTIONAL_HEADER_SIZE_OFFSET: usize = 16;
const PE32_MAGIC: u16 = 0x010b;
const PE32_IMAGE_BASE_OFFSET: usize = 28;
const PE32_MIN_OPTIONAL_HEADER_SIZE: usize = PE32_IMAGE_BASE_OFFSET + U32_SIZE;
const SECTION_HEADER_SIZE: usize = 40;
const SECTION_NAME_SIZE: usize = 8;
const SECTION_VIRTUAL_SIZE_OFFSET: usize = 8;
const SECTION_RVA_OFFSET: usize = 12;
const SECTION_RAW_SIZE_OFFSET: usize = 16;
const SECTION_RAW_FILE_OFFSET: usize = 20;
const MIN_FILE_REFERENCE_WORD: u16 = 0x0100;

#[derive(Debug, Clone)]
pub(crate) struct PeSection {
    pub(crate) name: String,
    rva: u32,
    virtual_size: u32,
    raw_file_offset: u32,
    raw_size: u32,
}

impl PeSection {
    pub(crate) fn raw_range(&self) -> Range<usize> {
        let start = self.raw_file_offset as usize;
        start..start + self.raw_size as usize
    }

    fn mapped_size(&self) -> u32 {
        self.virtual_size.max(self.raw_size)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PeImage<'data> {
    data: &'data [u8],
    image_base: u32,
    sections: Vec<PeSection>,
}

impl<'data> PeImage<'data> {
    pub(crate) fn parse(data: &'data [u8]) -> anyhow::Result<Self> {
        if data.len() < DOS_HEADER_MIN_SIZE {
            bail!("PE file too small");
        }
        if data.get(..DOS_SIGNATURE.len()) != Some(DOS_SIGNATURE.as_slice()) {
            bail!("invalid PE DOS signature");
        }

        let pe_offset = read_u32(data, DOS_E_LFANEW_OFFSET)? as usize;
        let signature_end = pe_offset
            .checked_add(PE_SIGNATURE.len())
            .context("PE signature offset overflow")?;
        if data.get(pe_offset..signature_end) != Some(PE_SIGNATURE.as_slice()) {
            bail!("invalid PE signature");
        }

        let coff_offset = signature_end;
        let optional_offset = coff_offset
            .checked_add(COFF_HEADER_SIZE)
            .context("PE optional header offset overflow")?;
        let coff_header = data
            .get(coff_offset..optional_offset)
            .context("PE COFF header truncated")?;

        let section_count = read_u16(coff_header, COFF_SECTION_COUNT_OFFSET)? as usize;
        let optional_header_size =
            read_u16(coff_header, COFF_OPTIONAL_HEADER_SIZE_OFFSET)? as usize;
        if optional_header_size < PE32_MIN_OPTIONAL_HEADER_SIZE {
            bail!(
                "PE32 optional header is too small: {optional_header_size} bytes, expected at least {PE32_MIN_OPTIONAL_HEADER_SIZE}"
            );
        }
        let optional_header_end = optional_offset
            .checked_add(optional_header_size)
            .context("PE optional header end overflow")?;
        let optional_header = data
            .get(optional_offset..optional_header_end)
            .context("PE optional header truncated")?;
        let optional_magic = read_u16(optional_header, 0)?;
        if optional_magic != PE32_MAGIC {
            bail!(
                "unsupported PE optional header magic 0x{optional_magic:04x}; expected PE32 0x{PE32_MAGIC:04x}"
            );
        }
        let image_base = read_u32(optional_header, PE32_IMAGE_BASE_OFFSET)?;

        let section_offset = optional_header_end;
        let section_table_bytes = section_count
            .checked_mul(SECTION_HEADER_SIZE)
            .context("PE section table size overflow")?;
        let section_table_end = section_offset
            .checked_add(section_table_bytes)
            .context("PE section table end overflow")?;
        data.get(section_offset..section_table_end)
            .context("PE section table truncated")?;

        let mut sections = Vec::with_capacity(section_count);
        for index in 0..section_count {
            let offset = section_offset + index * SECTION_HEADER_SIZE;
            let mut name_bytes = [0_u8; SECTION_NAME_SIZE];
            name_bytes.copy_from_slice(
                data.get(offset..offset + SECTION_NAME_SIZE)
                    .context("PE section name truncated")?,
            );
            let name = name_bytes
                .split(|&byte| byte == 0)
                .next()
                .unwrap_or(&name_bytes);
            let virtual_size = read_u32(data, offset + SECTION_VIRTUAL_SIZE_OFFSET)?;
            let rva = read_u32(data, offset + SECTION_RVA_OFFSET)?;
            let raw_size = read_u32(data, offset + SECTION_RAW_SIZE_OFFSET)?;
            let raw_file_offset = read_u32(data, offset + SECTION_RAW_FILE_OFFSET)?;
            let raw_end = u64::from(raw_file_offset)
                .checked_add(u64::from(raw_size))
                .context("PE section raw range overflow")?;
            if raw_end > data.len() as u64 {
                bail!(
                    "PE section {} raw range {}..{} exceeds file size {}",
                    String::from_utf8_lossy(name),
                    raw_file_offset,
                    raw_end,
                    data.len()
                );
            }

            sections.push(PeSection {
                name: String::from_utf8_lossy(name).into_owned(),
                rva,
                virtual_size,
                raw_file_offset,
                raw_size,
            });
        }

        Ok(Self {
            data,
            image_base,
            sections,
        })
    }

    pub(crate) fn data(&self) -> &'data [u8] {
        self.data
    }

    pub(crate) fn sections(&self) -> &[PeSection] {
        &self.sections
    }

    fn backed_file_range(&self, va: u32, byte_len: usize) -> Option<Range<usize>> {
        let rva = u64::from(va.checked_sub(self.image_base)?);
        let byte_len = u64::try_from(byte_len).ok()?;
        self.sections.iter().find_map(|section| {
            let section_rva = u64::from(section.rva);
            let delta = rva.checked_sub(section_rva)?;
            if delta >= u64::from(section.mapped_size()) {
                return None;
            }
            let raw_delta_end = delta.checked_add(byte_len)?;
            if raw_delta_end > u64::from(section.raw_size) {
                return None;
            }
            let file_start = u64::from(section.raw_file_offset).checked_add(delta)?;
            let file_end = file_start.checked_add(byte_len)?;
            if file_end > self.data.len() as u64 {
                return None;
            }
            Some(usize::try_from(file_start).ok()?..usize::try_from(file_end).ok()?)
        })
    }

    fn va_to_file_range(&self, va: u32, byte_len: usize) -> anyhow::Result<Range<usize>> {
        self.backed_file_range(va, byte_len)
            .with_context(|| format!("PE VA 0x{va:x} is not backed by {byte_len} file bytes"))
    }

    fn read_u32_at(&self, offset: usize) -> anyhow::Result<u32> {
        read_u32(self.data, offset)
    }

    fn read_u32_va(&self, va: u32) -> anyhow::Result<u32> {
        let offset = self.va_to_file_range(va, U32_SIZE)?.start;
        self.read_u32_at(offset)
    }

    pub(crate) fn locate_language_file_id_table(
        &self,
        files_per_language: usize,
        language_count: usize,
    ) -> anyhow::Result<u32> {
        let required = files_per_language
            .checked_mul(language_count)
            .context("PE language table size overflow")?;
        if required == 0 {
            bail!("PE language table cannot be empty");
        }

        let mut candidates = Vec::new();
        for section in &self.sections {
            let raw_range = section.raw_range();
            let start = raw_range.start;
            let end = raw_range.end;
            let mut run_start = start;
            let mut run_len = 0;

            for offset in (start..end).step_by(U32_SIZE) {
                let valid = read_u32(self.data, offset)
                    .ok()
                    .and_then(|pointer| self.backed_file_range(pointer, U32_SIZE))
                    .and_then(|range| read_u32(self.data, range.start).ok())
                    .is_some_and(|reference| {
                        reference as u16 >= MIN_FILE_REFERENCE_WORD
                            && (reference >> 16) as u16 >= MIN_FILE_REFERENCE_WORD
                    });

                if valid {
                    if run_len == 0 {
                        run_start = offset;
                    }
                    run_len += 1;
                    continue;
                }

                if run_len == required {
                    let delta = u32::try_from(run_start - start)
                        .context("PE language table offset exceeds u32")?;
                    candidates.push(
                        self.image_base
                            .checked_add(section.rva)
                            .and_then(|va| va.checked_add(delta))
                            .context("PE language table VA overflow")?,
                    );
                }
                run_len = 0;
            }

            if run_len == required {
                let delta = u32::try_from(run_start - start)
                    .context("PE language table offset exceeds u32")?;
                candidates.push(
                    self.image_base
                        .checked_add(section.rva)
                        .and_then(|va| va.checked_add(delta))
                        .context("PE language table VA overflow")?,
                );
            }
        }

        match candidates.as_slice() {
            [table_va] => Ok(*table_va),
            [] => bail!("client language file-ID table not found in PE"),
            _ => bail!("client language file-ID table is ambiguous in PE"),
        }
    }

    pub(crate) fn language_file_ids(
        &self,
        table_va: u32,
        files_per_language: usize,
        language_index: usize,
    ) -> anyhow::Result<Vec<Option<u32>>> {
        let row_size = files_per_language
            .checked_mul(U32_SIZE)
            .context("PE language row size overflow")?;
        let row_delta = language_index
            .checked_mul(row_size)
            .context("PE language row offset overflow")?;
        let row_va = table_va
            .checked_add(u32::try_from(row_delta).context("PE language row offset exceeds u32")?)
            .context("PE language row VA overflow")?;
        let row_offset = self.va_to_file_range(row_va, row_size)?.start;
        let mut file_ids = Vec::with_capacity(files_per_language);
        for index in 0..files_per_language {
            let ptr_offset = row_offset
                .checked_add(
                    index
                        .checked_mul(U32_SIZE)
                        .context("PE language file index overflow")?,
                )
                .context("PE language pointer offset overflow")?;
            let ptr = self
                .read_u32_at(ptr_offset)
                .with_context(|| format!("language {language_index} text-file pointer {index}"))?;
            if ptr == 0 {
                file_ids.push(None);
                continue;
            }
            let raw_ref = self.read_u32_va(ptr).with_context(|| {
                format!("language {language_index} text-file reference {index}")
            })?;
            file_ids.push(Some(decode_file_reference(
                raw_ref as u16,
                (raw_ref >> 16) as u16,
            )));
        }

        Ok(file_ids)
    }
}

#[cfg(test)]
mod tests {
    use super::PeImage;
    use crate::file_ref::encode_file_reference;

    const IMAGE_BASE: u32 = 0x0040_0000;
    const SECTION_VA: u32 = 0x1000;
    const SECTION_RAW_OFFSET: usize = 0x300;
    const PE_OFFSET: usize = 0x80;
    const COFF_OFFSET: usize = PE_OFFSET + 4;
    const OPTIONAL_OFFSET: usize = COFF_OFFSET + 20;
    const OPTIONAL_SIZE: usize = 0xe0;
    const SECTION_OFFSET: usize = OPTIONAL_OFFSET + OPTIONAL_SIZE;

    fn write_u16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn test_pe() -> Vec<u8> {
        let mut bytes = vec![0_u8; 0x500];
        bytes[..2].copy_from_slice(b"MZ");
        write_u32(&mut bytes, 0x3c, PE_OFFSET as u32);
        bytes[PE_OFFSET..PE_OFFSET + 4].copy_from_slice(b"PE\0\0");
        write_u16(&mut bytes, COFF_OFFSET + 2, 1);
        write_u16(&mut bytes, COFF_OFFSET + 16, OPTIONAL_SIZE as u16);
        write_u16(&mut bytes, OPTIONAL_OFFSET, 0x010b);
        write_u32(&mut bytes, OPTIONAL_OFFSET + 28, IMAGE_BASE);

        bytes[SECTION_OFFSET..SECTION_OFFSET + 8].copy_from_slice(b".rdata\0\0");
        write_u32(&mut bytes, SECTION_OFFSET + 8, 0x180);
        write_u32(&mut bytes, SECTION_OFFSET + 12, SECTION_VA);
        write_u32(&mut bytes, SECTION_OFFSET + 16, 0x100);
        write_u32(&mut bytes, SECTION_OFFSET + 20, SECTION_RAW_OFFSET as u32);
        bytes
    }

    #[test]
    fn rejects_invalid_pe32_headers() {
        let mut invalid_dos = test_pe();
        invalid_dos[0] = 0;
        assert!(PeImage::parse(&invalid_dos).is_err());

        let mut short_optional_header = test_pe();
        write_u16(&mut short_optional_header, COFF_OFFSET + 16, 31);
        assert!(PeImage::parse(&short_optional_header).is_err());

        let mut pe32_plus = test_pe();
        write_u16(&mut pe32_plus, OPTIONAL_OFFSET, 0x020b);
        assert!(PeImage::parse(&pe32_plus).is_err());
    }

    #[test]
    fn maps_virtual_addresses_and_resolves_language_file_references() -> anyhow::Result<()> {
        let mut bytes = test_pe();
        let reference_file_id = 123_456;
        let reference_va = IMAGE_BASE + SECTION_VA + 0x20;
        let (id0, id1) = encode_file_reference(reference_file_id);

        write_u32(&mut bytes, SECTION_RAW_OFFSET, reference_va);
        write_u32(&mut bytes, SECTION_RAW_OFFSET + 4, 0);
        write_u32(
            &mut bytes,
            SECTION_RAW_OFFSET + 0x20,
            u32::from(id0) | (u32::from(id1) << 16),
        );

        let pe = PeImage::parse(&bytes)?;
        assert_eq!(
            pe.va_to_file_range(reference_va, 1)?.start,
            SECTION_RAW_OFFSET + 0x20
        );
        assert_eq!(
            pe.language_file_ids(IMAGE_BASE + SECTION_VA, 2, 0)?,
            vec![Some(reference_file_id), None]
        );
        Ok(())
    }

    #[test]
    fn locates_relocated_language_file_id_table() -> anyhow::Result<()> {
        let mut bytes = test_pe();
        let table_offset = SECTION_RAW_OFFSET + 0x40;
        let reference_offset = SECTION_RAW_OFFSET + 0x80;

        for index in 0..4 {
            let reference_va =
                IMAGE_BASE + SECTION_VA + u32::try_from(reference_offset - SECTION_RAW_OFFSET)?;
            write_u32(
                &mut bytes,
                table_offset + index * 4,
                reference_va + (index * 4) as u32,
            );
            let (id0, id1) = encode_file_reference(123_456 + index as u32);
            write_u32(
                &mut bytes,
                reference_offset + index * 4,
                u32::from(id0) | (u32::from(id1) << 16),
            );
        }

        let pe = PeImage::parse(&bytes)?;
        let table_va = pe.locate_language_file_id_table(2, 2)?;
        assert_eq!(
            table_va,
            IMAGE_BASE + SECTION_VA + u32::try_from(table_offset - SECTION_RAW_OFFSET)?
        );
        assert_eq!(
            pe.language_file_ids(table_va, 2, 1)?,
            vec![Some(123_458), Some(123_459)]
        );
        Ok(())
    }

    #[test]
    fn rejects_virtual_addresses_outside_backed_section_data() -> anyhow::Result<()> {
        let bytes = test_pe();
        let pe = PeImage::parse(&bytes)?;

        assert!(pe.va_to_file_range(IMAGE_BASE - 1, 1).is_err());
        assert!(
            pe.va_to_file_range(IMAGE_BASE + SECTION_VA + 0x100, 1)
                .is_err()
        );
        assert!(
            pe.va_to_file_range(IMAGE_BASE + SECTION_VA + 0x180, 1)
                .is_err()
        );
        assert!(
            pe.language_file_ids(IMAGE_BASE + SECTION_VA + 0xfc, 2, 0)
                .is_err()
        );
        Ok(())
    }
}
