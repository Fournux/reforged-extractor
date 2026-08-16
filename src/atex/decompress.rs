use std::mem::size_of;

use anyhow::{Context, bail};

use super::{
    ATEX_ENCODED_HEADER_LEN, ATEX_HEADER_LEN, AtexDxtFormat, AtexHeader, DXT_BLOCK_SIDE,
    dxt_block_count,
};

const WORD_BYTES: usize = size_of::<u32>();
const DATA_SIZE_WORD: usize = ATEX_HEADER_LEN / WORD_BYTES;
const COMPRESSION_CODE_WORD: usize = DATA_SIZE_WORD + 1;
const BITSTREAM_WORD: usize = ATEX_ENCODED_HEADER_LEN / WORD_BYTES;
const MIN_DATA_RANGE_SIZE: usize = ATEX_ENCODED_HEADER_LEN - ATEX_HEADER_LEN;

const SUBCODE_DXT1_CONSTANT_BLOCKS: u32 = 0x01;
const SUBCODE_DXT3_ALPHA_BLOCKS: u32 = 0x02;
const SUBCODE_DXT5_ALPHA_BLOCKS: u32 = 0x04;
const SUBCODE_COLOR_BLOCKS: u32 = 0x08;
const SUBCODE_SWIZZLED_BORDERS: u32 = 0x10;

const SWIZZLED_TEXTURE_SIDE: u16 = 256;
const SWIZZLED_BLOCKS_PER_ROW: usize = SWIZZLED_TEXTURE_SIDE as usize / DXT_BLOCK_SIDE;
const SWIZZLED_BORDER_MASK: u32 = 0xc000_0003;

const HUFFMAN_RUNS: [u8; 128] = [
    0x6, 0x10, 0x6, 0x0f, 0x6, 0x0e, 0x6, 0x0d, 0x6, 0x0c, 0x6, 0x0b, 0x6, 0x0a, 0x6, 0x9, 0x6,
    0x8, 0x6, 0x7, 0x6, 0x6, 0x6, 0x5, 0x6, 0x4, 0x6, 0x3, 0x6, 0x2, 0x6, 0x1, 0x2, 0x11, 0x2,
    0x11, 0x2, 0x11, 0x2, 0x11, 0x2, 0x11, 0x2, 0x11, 0x2, 0x11, 0x2, 0x11, 0x2, 0x11, 0x2, 0x11,
    0x2, 0x11, 0x2, 0x11, 0x2, 0x11, 0x2, 0x11, 0x2, 0x11, 0x2, 0x11, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0,
    0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1,
    0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0,
    0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1,
    0x0,
];

pub(super) fn decompress_atex_blocks(
    atex_bytes: &[u8],
    header: AtexHeader,
) -> anyhow::Result<Vec<u8>> {
    if atex_bytes.len() < ATEX_ENCODED_HEADER_LEN {
        bail!("ATEX payload too small");
    }

    let word_count = atex_bytes.len() / WORD_BYTES;
    let data_size = usize::try_from(
        read_word(atex_bytes, DATA_SIZE_WORD).context("ATEX missing data-range size")?,
    )
    .context("ATEX data-range size does not fit usize")?;
    let compression_code =
        read_word(atex_bytes, COMPRESSION_CODE_WORD).context("ATEX missing compression code")?;
    let block_count = dxt_block_count(header.width as usize, header.height as usize, "ATEX")?;
    let (alpha_words, color_words) = header.format.block_layout();
    let block_words = alpha_words + color_words;
    if !matches!(block_words, 2 | 4) {
        bail!("unsupported ATEX block layout size {block_words}");
    }
    let dxt_len = block_count
        .checked_mul(block_words)
        .and_then(|words| words.checked_mul(WORD_BYTES))
        .context("ATEX DXT byte count overflow")?;

    let swizzled_borders = compression_code & SUBCODE_SWIZZLED_BORDERS != 0
        && header.width == SWIZZLED_TEXTURE_SIDE
        && header.height == SWIZZLED_TEXTURE_SIDE
        && matches!(
            header.format,
            AtexDxtFormat::Dxt2 | AtexDxtFormat::Dxt3 | AtexDxtFormat::DxtN
        );

    let end_byte = ATEX_HEADER_LEN
        .checked_add(data_size)
        .context("ATEX compressed data range overflow")?;
    let end_word = end_byte / WORD_BYTES;
    if end_word > word_count || data_size <= MIN_DATA_RANGE_SIZE {
        bail!("invalid ATEX compressed data range");
    }

    let mut bits = BitReader::new(atex_bytes, BITSTREAM_WORD, end_word)?;
    let mut out = vec![0_u8; dxt_len];
    let bitmap_len = block_count.div_ceil(u32::BITS as usize);
    let mut dcmp1 = vec![0_u32; bitmap_len];
    let mut dcmp2 = vec![0_u32; bitmap_len];
    if swizzled_borders {
        subcode1(&mut dcmp1, &mut dcmp2, block_count);
    }

    if compression_code & SUBCODE_DXT1_CONSTANT_BLOCKS != 0 && header.format == AtexDxtFormat::Dxt1
    {
        subcode2(
            &mut out,
            &mut dcmp1,
            &mut dcmp2,
            &mut bits,
            block_count,
            block_words,
        )?;
    }
    if compression_code & SUBCODE_DXT3_ALPHA_BLOCKS != 0
        && matches!(
            header.format,
            AtexDxtFormat::Dxt2 | AtexDxtFormat::Dxt3 | AtexDxtFormat::DxtN
        )
    {
        subcode3(&mut out, &mut dcmp1, &mut bits, block_count, block_words)?;
    }
    if compression_code & SUBCODE_DXT5_ALPHA_BLOCKS != 0
        && matches!(
            header.format,
            AtexDxtFormat::Dxt4 | AtexDxtFormat::Dxt5 | AtexDxtFormat::DxtA | AtexDxtFormat::DxtL
        )
    {
        subcode4(
            &mut out,
            &mut dcmp1,
            &dcmp2,
            &mut bits,
            block_count,
            block_words,
        )?;
    }
    if compression_code & SUBCODE_COLOR_BLOCKS != 0 {
        if color_words == 0 {
            bail!("ATEX color subcode used by a format without color blocks");
        }
        subcode5(
            &mut out,
            &mut dcmp2,
            &mut bits,
            block_count,
            block_words,
            header.format == AtexDxtFormat::Dxt1,
        )?;
    }

    let mut pos = bits.tail_word();
    if alpha_words > 0 {
        let raw_alpha_words = (0..block_count)
            .filter(|block| dcmp1[*block >> 5] & (1 << (*block & 31)) == 0)
            .count()
            * 2;
        let tail_cannot_hold_alpha = pos > end_word || raw_alpha_words > end_word - pos;
        let dxt3_opaque_alpha_fallback = matches!(
            header.format,
            AtexDxtFormat::Dxt2 | AtexDxtFormat::Dxt3 | AtexDxtFormat::DxtN
        ) && tail_cannot_hold_alpha;
        for block in 0..block_count {
            if dcmp1[block >> 5] & (1 << (block & 31)) == 0 {
                let dst = block * block_words;
                if dxt3_opaque_alpha_fallback {
                    write_word(&mut out, dst, u32::MAX);
                    write_word(&mut out, dst + 1, u32::MAX);
                } else {
                    let alpha0 = read_word(atex_bytes, pos).context("ATEX alpha block underrun")?;
                    let alpha1 =
                        read_word(atex_bytes, pos + 1).context("ATEX alpha block underrun")?;
                    write_word(&mut out, dst, alpha0);
                    write_word(&mut out, dst + 1, alpha1);
                    pos += 2;
                }
            }
        }
    }
    if color_words > 0 {
        for block in 0..block_count {
            if dcmp2[block >> 5] & (1 << (block & 31)) == 0 {
                let dst = block * block_words + alpha_words;
                let color = read_word(atex_bytes, pos).context("ATEX color block underrun")?;
                write_word(&mut out, dst, color);
                pos += 1;
            }
        }
        for block in 0..block_count {
            if dcmp2[block >> 5] & (1 << (block & 31)) == 0 {
                let dst = block * block_words + alpha_words + 1;
                let indices =
                    read_word(atex_bytes, pos).context("ATEX color index block underrun")?;
                write_word(&mut out, dst, indices);
                pos += 1;
            }
        }
    }

    if swizzled_borders {
        subcode7(&mut out, block_count, block_words)?;
    }

    Ok(out)
}

fn read_word(bytes: &[u8], index: usize) -> Option<u32> {
    let offset = index.checked_mul(WORD_BYTES)?;
    let end = offset.checked_add(WORD_BYTES)?;
    Some(u32::from_le_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

fn write_word(bytes: &mut [u8], index: usize, value: u32) {
    let offset = index * WORD_BYTES;
    bytes[offset..offset + WORD_BYTES].copy_from_slice(&value.to_le_bytes());
}

#[derive(Clone, Copy)]
struct BitReader<'a> {
    bytes: &'a [u8],
    word: usize,
    bit_offset: u32,
    end_word: usize,
}

impl<'a> BitReader<'a> {
    fn new(bytes: &'a [u8], word: usize, end_word: usize) -> anyhow::Result<Self> {
        if word > end_word || end_word > bytes.len() / WORD_BYTES {
            bail!("invalid ATEX bitstream range");
        }
        Ok(Self {
            bytes,
            word,
            bit_offset: 0,
            end_word,
        })
    }

    fn tail_word(self) -> usize {
        self.word + usize::from(self.bit_offset != 0)
    }

    fn available_bits(self) -> usize {
        self.end_word
            .saturating_sub(self.word)
            .saturating_mul(u32::BITS as usize)
            .saturating_sub(self.bit_offset as usize)
    }

    fn bit(&mut self) -> anyhow::Result<u32> {
        self.take_bits(1)
    }

    fn take_bits(&mut self, bits: u32) -> anyhow::Result<u32> {
        if bits > u32::BITS {
            bail!("ATEX bit read exceeds 32 bits");
        }
        if self.available_bits() < bits as usize {
            bail!("ATEX compressed bitstream underrun");
        }
        Ok(self.take_available_bits(bits))
    }

    fn peek_bits_padded(self, bits: u32) -> u32 {
        debug_assert!(bits <= u32::BITS);
        let available = bits.min(self.available_bits() as u32);
        let mut reader = self;
        let value = reader.take_available_bits(available);
        value.checked_shl(bits - available).unwrap_or(0)
    }

    fn take_huffman_run(&mut self) -> anyhow::Result<usize> {
        let offset = self.peek_bits_padded(6) as usize * 2;
        let bit_count = u32::from(HUFFMAN_RUNS[offset]);
        let run_length = usize::from(HUFFMAN_RUNS[offset + 1]) + 1;
        self.take_bits(bit_count)?;
        Ok(run_length)
    }

    fn take_available_bits(&mut self, bits: u32) -> u32 {
        let mut value = 0_u32;
        let mut remaining = bits;
        while remaining > 0 {
            let word = read_word(self.bytes, self.word).expect("validated ATEX bitstream word");
            let available = u32::BITS - self.bit_offset;
            let take = remaining.min(available);
            let shift = available - take;
            let piece = if take == u32::BITS {
                word
            } else {
                (word >> shift) & ((1_u32 << take) - 1)
            };
            value = if take == u32::BITS {
                piece
            } else {
                (value << take) | piece
            };
            self.bit_offset += take;
            if self.bit_offset == u32::BITS {
                self.word += 1;
                self.bit_offset = 0;
            }
            remaining -= take;
        }
        value
    }
}

fn subcode1(dcmp1: &mut [u32], dcmp2: &mut [u32], block_count: usize) {
    for block in 0..block_count {
        let mask = 1_u32 << (block & 31);
        let row_mask = 1_u32 << ((block / SWIZZLED_BLOCKS_PER_ROW) & (u32::BITS as usize - 1));
        if mask & SWIZZLED_BORDER_MASK != 0 || row_mask & SWIZZLED_BORDER_MASK != 0 {
            dcmp1[block >> 5] |= mask;
            dcmp2[block >> 5] |= mask;
        }
    }
}

fn subcode2(
    out: &mut [u8],
    dcmp1: &mut [u32],
    dcmp2: &mut [u32],
    bits: &mut BitReader<'_>,
    block_count: usize,
    block_words: usize,
) -> anyhow::Result<()> {
    let mut block = 0;
    while block < block_count {
        let read_count = bits.take_huffman_run()?;
        let fill_block = bits.bit()? != 0;
        let mut remaining = read_count;
        while remaining > 0 && block < block_count {
            let mask = 1_u32 << (block & 31);
            let word = block >> 5;
            if dcmp2[word] & mask == 0 {
                if fill_block {
                    let dst = block * block_words;
                    write_word(out, dst, 0xffff_fffe);
                    write_word(out, dst + 1, 0xffff_ffff);
                    dcmp1[word] |= mask;
                    dcmp2[word] |= mask;
                }
                remaining -= 1;
            }
            block += 1;
        }
        while block < block_count {
            let mask = 1_u32 << (block & 31);
            if dcmp2[block >> 5] & mask == 0 {
                break;
            }
            block += 1;
        }
    }
    Ok(())
}

fn subcode3(
    out: &mut [u8],
    dcmp1: &mut [u32],
    bits: &mut BitReader<'_>,
    block_count: usize,
    block_words: usize,
) -> anyhow::Result<()> {
    let alpha_nibble = bits.take_bits(4)?;
    let alpha_byte = (alpha_nibble << 4) | alpha_nibble;
    let alpha_pattern = alpha_byte | (alpha_byte << 8) | (alpha_byte << 16) | (alpha_byte << 24);
    let alpha_table = [0, 0, 0, 0, alpha_pattern, alpha_pattern, 0, 0];

    let mut block = 0;
    while block < block_count {
        let read_count = bits.take_huffman_run()?;
        let flag1 = bits.bit()?;
        let alpha_index = if flag1 == 0 { 0 } else { flag1 + bits.bit()? } as usize;
        let mut remaining = read_count;
        while remaining > 0 && block < block_count {
            let mask = 1_u32 << (block & 31);
            let word = block >> 5;
            if dcmp1[word] & mask == 0 {
                if alpha_index != 0 {
                    let dst = block * block_words;
                    write_word(out, dst, alpha_table[alpha_index * 2]);
                    write_word(out, dst + 1, alpha_table[alpha_index * 2 + 1]);
                    dcmp1[word] |= mask;
                }
                remaining -= 1;
            }
            block += 1;
        }
        while block < block_count {
            let mask = 1_u32 << (block & 31);
            if dcmp1[block >> 5] & mask == 0 {
                break;
            }
            block += 1;
        }
    }
    Ok(())
}

fn subcode4(
    out: &mut [u8],
    dcmp1: &mut [u32],
    dcmp2: &[u32],
    bits: &mut BitReader<'_>,
    block_count: usize,
    block_words: usize,
) -> anyhow::Result<()> {
    let extracted = bits.take_bits(8)?;
    let color_pattern = (extracted << 8) | extracted;
    let color_table = [0, 0, 0, 0, color_pattern, 0, 0, 0];
    let mut block = 0;
    while block < block_count {
        let read_count = bits.take_huffman_run()?;
        let flag1 = bits.bit()?;
        let color_index = if flag1 == 0 { 0 } else { flag1 + bits.bit()? } as usize;
        let mut remaining = read_count;
        while remaining > 0 && block < block_count {
            let mask = 1_u32 << (block & 31);
            let word = block >> 5;
            if dcmp2[word] & mask == 0 {
                if color_index != 0 {
                    let dst = block * block_words;
                    write_word(out, dst, color_table[color_index * 2]);
                    write_word(out, dst + 1, color_table[color_index * 2 + 1]);
                    dcmp1[word] |= mask;
                }
                remaining -= 1;
            }
            block += 1;
        }
        while block < block_count {
            let mask = 1_u32 << (block & 31);
            if dcmp2[block >> 5] & mask == 0 {
                break;
            }
            block += 1;
        }
    }
    Ok(())
}

fn subcode5(
    out: &mut [u8],
    dcmp2: &mut [u32],
    bits: &mut BitReader<'_>,
    block_count: usize,
    block_words: usize,
    dxt1_flag: bool,
) -> anyhow::Result<()> {
    let color_value = bits.take_bits(24)? | 0xff00_0000;
    let color_data = subcode6(color_value, dxt1_flag);
    let mut block = 0;
    while block < block_count {
        let read_count = bits.take_huffman_run()?;
        let bit = bits.bit()?;
        let mut remaining = read_count;
        while remaining > 0 && block < block_count {
            let mask = 1_u32 << (block & 31);
            let word = block >> 5;
            if dcmp2[word] & mask == 0 {
                if bit != 0 {
                    let color_offset = if dxt1_flag { 0 } else { 2 };
                    let dst = block * block_words + color_offset;
                    write_word(out, dst, color_data[0]);
                    write_word(out, dst + 1, color_data[1]);
                    dcmp2[word] |= mask;
                }
                remaining -= 1;
            }
            block += 1;
        }
        while block < block_count {
            let mask = 1_u32 << (block & 31);
            if dcmp2[block >> 5] & mask == 0 {
                break;
            }
            block += 1;
        }
    }
    Ok(())
}

fn subcode6(color_value: u32, dxt1_flag: bool) -> [u32; 2] {
    let r = color_value & 0xff;
    let g = (color_value >> 8) & 0xff;
    let b = (color_value >> 16) & 0xff;

    let bases = [
        (r - (r >> 5)) >> 3,
        (g - (g >> 6)) >> 2,
        (b - (b >> 5)) >> 3,
    ];
    let quantized = [
        (bases[0] >> 2) + bases[0] * 8,
        (bases[1] >> 4) + bases[1] * 4,
        (bases[2] >> 2) + bases[2] * 8,
    ];
    let next_bases = [bases[0] + 1, bases[1] + 1, bases[2] + 1];
    let next_quantized = [
        (next_bases[0] >> 2) + next_bases[0] * 8,
        (next_bases[1] >> 4) + next_bases[1] * 4,
        (next_bases[2] >> 2) + next_bases[2] * 8,
    ];
    let channels = [r, g, b];
    let mut values = [0_u32; 3];
    for i in 0..3 {
        let delta = next_quantized[i] - quantized[i];
        values[i] = (channels[i] * 12 - quantized[i] * 12)
            .checked_div(delta)
            .unwrap_or(0);
    }

    let mut palette = [(0_u32, 0_u32); 3];
    for i in 0..3 {
        palette[i] = match values[i] {
            0..=1 => (bases[i], bases[i]),
            2..=5 => (bases[i], bases[i] + 1),
            6..=9 => (bases[i] + 1, bases[i]),
            _ => (bases[i] + 1, bases[i] + 1),
        };
    }

    let mut color1 = palette[0].0 | (palette[1].0 << 5) | (palette[2].0 << 11);
    let mut color2 = palette[0].1 | (palette[1].1 << 5) | (palette[2].1 << 11);
    let mut score = 0;
    let mut count = 0;
    for i in 0..3 {
        if palette[i].0 != palette[i].1 {
            score += if palette[i].0 == bases[i] {
                values[i]
            } else {
                12 - values[i]
            };
            count += 1;
        }
    }
    let mut avg = (score + count / 2).checked_div(count).unwrap_or(0);
    let swap = dxt1_flag && ((avg == 5 || avg == 6) || count == 0);
    if count == 0 && !swap {
        if color2 != 0xffff {
            avg = 0;
            color2 += 1;
        } else {
            avg = 12;
            color1 -= 1;
        }
    }
    if (color1 < color2) != swap {
        std::mem::swap(&mut color1, &mut color2);
        avg = 12 - avg;
    }

    let table = if swap {
        2
    } else if avg < 2 {
        0
    } else if avg < 6 {
        2
    } else if avg < 10 {
        3
    } else {
        1
    };
    let mut pattern = table * 5;
    pattern = (pattern << 4) | pattern;
    pattern = (pattern << 8) | pattern;
    pattern = (pattern << 16) | pattern;
    [(color2 << 16) | color1, pattern]
}

fn subcode7(out: &mut [u8], block_count: usize, block_words: usize) -> anyhow::Result<()> {
    if block_words != 4 {
        bail!("ATEX border unswizzle requires four words per block");
    }
    for block in 0..block_count {
        let position_in_row = block % SWIZZLED_BLOCKS_PER_ROW;
        let row = block / SWIZZLED_BLOCKS_PER_ROW;
        let low_swizzle = (1_u32 << (position_in_row & 31)) & SWIZZLED_BORDER_MASK != 0;
        let high_swizzle = (1_u32 << (row & 31)) & SWIZZLED_BORDER_MASK != 0;
        if !low_swizzle && !high_swizzle {
            continue;
        }

        let source_in_row = if low_swizzle {
            position_in_row ^ 3
        } else {
            position_in_row
        };
        let source_row = if high_swizzle { row ^ 3 } else { row };
        let source = source_row * SWIZZLED_BLOCKS_PER_ROW + source_in_row;
        if source >= block_count {
            continue;
        }
        let source_offset = source * block_words;
        let mut data0 =
            read_word(out, source_offset).context("ATEX border source block out of bounds")?;
        let mut data1 =
            read_word(out, source_offset + 1).context("ATEX border source block out of bounds")?;
        let data2 =
            read_word(out, source_offset + 2).context("ATEX border source block out of bounds")?;
        let mut data3 =
            read_word(out, source_offset + 3).context("ATEX border source block out of bounds")?;

        if low_swizzle {
            for _ in 0..2 {
                let mixed_high = ((data0 >> 8) & 0x00f0_00f0) | (data0 & 0x0f00_0f00);
                let mixed_low = ((data0 & 0xffff_000f) << 8) | (data0 & 0x00f0_00f0);
                data0 = (mixed_high >> 4) | (mixed_low << 4);
            }
            let low = ((data3 & 0xff03_0303) << 4) | (data3 & 0x0c0c_0c0c);
            let high = ((data3 >> 4) & 0x0c0c_0c0c) | (data3 & 0x3030_3030);
            data3 = (low << 2) | (high >> 2);
        }
        if high_swizzle {
            let old_data0 = data0;
            data0 = data1.rotate_left(16);
            data1 = old_data0.rotate_left(16);
            let low = (data3 & 0x00ff_0000) | (data3 >> 16);
            let high = (data3 << 16) | (data3 & 0x0000_ff00);
            data3 = (low >> 8) | (high << 8);
        }

        let destination = block * block_words;
        for (offset, word) in [data0, data1, data2, data3].into_iter().enumerate() {
            write_word(out, destination + offset, word);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subcode2_marks_dxt1_constant_blocks() {
        let bytes = u32::MAX.to_le_bytes();
        let mut bits = BitReader::new(&bytes, 0, 1).unwrap();
        let mut out = vec![0_u8; 2 * WORD_BYTES];
        let mut dcmp1 = vec![0_u32; 1];
        let mut dcmp2 = vec![0_u32; 1];

        subcode2(&mut out, &mut dcmp1, &mut dcmp2, &mut bits, 1, 2).unwrap();

        assert_eq!(read_word(&out, 0), Some(0xffff_fffe));
        assert_eq!(read_word(&out, 1), Some(0xffff_ffff));
        assert_eq!(dcmp1[0], 1);
        assert_eq!(dcmp2[0], 1);
    }

    #[test]
    fn bit_reader_rejects_an_actual_underrun() {
        let bytes = u32::MAX.to_le_bytes();
        let mut bits = BitReader::new(&bytes, 0, 1).unwrap();

        assert_eq!(bits.take_bits(32).unwrap(), u32::MAX);
        let err = bits
            .bit()
            .expect_err("reading beyond the bitstream must fail");

        assert!(err.to_string().contains("bitstream underrun"));
    }

    #[test]
    fn border_subcodes_mark_and_remap_swizzled_dxt3_blocks() {
        let mut dcmp1 = [0_u32; 8];
        let mut dcmp2 = [0_u32; 8];
        subcode1(&mut dcmp1, &mut dcmp2, 256);
        assert_ne!(dcmp1[128 >> 5] & (1 << (128 & 31)), 0);
        assert_eq!(dcmp1[130 >> 5] & (1 << (130 & 31)), 0);
        assert_ne!(dcmp2[158 >> 5] & (1 << (158 & 31)), 0);

        let mut out = Vec::with_capacity(256 * 4 * WORD_BYTES);
        for block in 0..256_u32 {
            for word in [block, block + 1_000, block + 2_000, block + 3_000] {
                out.extend_from_slice(&word.to_le_bytes());
            }
        }
        subcode7(&mut out, 256, 4).unwrap();

        assert_eq!(read_word(&out, 128 * 4 + 2), Some(131 + 2_000));
        assert_eq!(read_word(&out, 130 * 4 + 2), Some(130 + 2_000));
    }
}
