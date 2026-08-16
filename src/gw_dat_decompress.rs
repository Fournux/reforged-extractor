use anyhow::{Context, bail};

const MAX_DECOMPRESSED_SIZE: usize = 256 * 1024 * 1024;

const CODE_LENGTH_LOOKUP: [(u32, u32); 14] = [
    (0xA000_0000, 0x02),
    (0x6000_0000, 0x06),
    (0x4000_0000, 0x0A),
    (0x2000_0000, 0x12),
    (0x1200_0000, 0x19),
    (0x0C00_0000, 0x1F),
    (0x0700_0000, 0x29),
    (0x0300_0000, 0x39),
    (0x0160_0000, 0x46),
    (0x00F0_0000, 0x4D),
    (0x00C0_0000, 0x53),
    (0x00B0_0000, 0x57),
    (0x00A0_0000, 0x5F),
    (0x0000_0000, 0xFF),
];
const CODE_LENGTH_RUNS: [u8; 256] = [
    0x08, 0x09, 0x0A, 0x00, 0x07, 0x0B, 0x0C, 0x06, 0x29, 0x2A, 0xE0, 0x04, 0x05, 0x20, 0x28, 0x2B,
    0x2C, 0x40, 0x4A, 0x03, 0x0D, 0x25, 0x26, 0x27, 0x48, 0x49, 0x24, 0x47, 0x4B, 0x4C, 0x69, 0x6A,
    0x23, 0x46, 0x60, 0x63, 0x67, 0x68, 0x88, 0x89, 0xA0, 0xE8, 0x01, 0x02, 0x2D, 0x43, 0x44, 0x45,
    0x65, 0x66, 0x80, 0x87, 0x8A, 0xA8, 0xA9, 0xC0, 0xC9, 0xE9, 0x0E, 0x4D, 0x64, 0x6B, 0x6C, 0x84,
    0x85, 0x8B, 0xA4, 0xA5, 0xAA, 0xC8, 0xE5, 0x83, 0x86, 0xA6, 0xA7, 0xC7, 0xCA, 0xE7, 0x22, 0x2E,
    0x8C, 0xC4, 0xE4, 0xE6, 0x4E, 0x6D, 0xC6, 0xEC, 0x0F, 0x10, 0x11, 0x8D, 0xAB, 0xAC, 0xCC, 0xEA,
    0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x1B, 0x1C, 0x1D, 0x1E, 0x1F, 0x21, 0x2F,
    0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3A, 0x3B, 0x3C, 0x3D, 0x3E, 0x3F,
    0x41, 0x42, 0x4F, 0x50, 0x51, 0x52, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5A, 0x5B, 0x5C,
    0x5D, 0x5E, 0x5F, 0x61, 0x62, 0x6E, 0x6F, 0x70, 0x71, 0x72, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78,
    0x79, 0x7A, 0x7B, 0x7C, 0x7D, 0x7E, 0x7F, 0x81, 0x82, 0x8E, 0x8F, 0x90, 0x91, 0x92, 0x93, 0x94,
    0x95, 0x96, 0x97, 0x98, 0x99, 0x9A, 0x9B, 0x9C, 0x9D, 0x9E, 0x9F, 0xA1, 0xA2, 0xA3, 0xAD, 0xAE,
    0xAF, 0xB0, 0xB1, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6, 0xB7, 0xB8, 0xB9, 0xBA, 0xBB, 0xBC, 0xBD, 0xBE,
    0xBF, 0xC1, 0xC2, 0xC3, 0xC5, 0xCB, 0xCD, 0xCE, 0xCF, 0xD0, 0xD1, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6,
    0xD7, 0xD8, 0xD9, 0xDA, 0xDB, 0xDC, 0xDD, 0xDE, 0xDF, 0xE1, 0xE2, 0xE3, 0xEB, 0xED, 0xEE, 0xEF,
    0xF0, 0xF1, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7, 0xF8, 0xF9, 0xFA, 0xFB, 0xFC, 0xFD, 0xFE, 0xFF,
];
const LENGTH_BASES: [u8; 29] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 14, 16, 20, 24, 28, 32, 40, 48, 56, 64, 80, 96, 112, 128,
    160, 192, 224, 255,
];
const LENGTH_EXTRA_BITS: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DISTANCE_BASES: [u16; 30] = [
    0, 1, 2, 3, 4, 6, 8, 12, 16, 24, 32, 48, 64, 96, 128, 192, 256, 384, 512, 768, 1024, 1536,
    2048, 3072, 4096, 6144, 8192, 12288, 16384, 24576,
];
const DISTANCE_EXTRA_BITS: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
const INVALID: u32 = u32::MAX;

pub(crate) fn decompress_gw_dat(input: &[u8]) -> anyhow::Result<Vec<u8>> {
    Decompressor::new(input)?.decompress()
}

#[derive(Clone, Copy)]
struct HuffmanEntry {
    bit_count: u32,
    symbol: u32,
}

const EMPTY_HUFFMAN_ENTRY: HuffmanEntry = HuffmanEntry {
    bit_count: 0,
    symbol: INVALID,
};

#[derive(Clone, Copy, Default)]
struct HuffmanHelper {
    threshold: u32,
    last_symbol: u32,
    bit_count: u32,
}

struct HuffmanData {
    fast: [HuffmanEntry; 0x100],
    helpers: [HuffmanHelper; 0x18],
    helper_len: usize,
    long_symbols: Vec<u32>,
}

impl Default for HuffmanData {
    fn default() -> Self {
        Self {
            fast: [EMPTY_HUFFMAN_ENTRY; 0x100],
            helpers: [HuffmanHelper::default(); 0x18],
            helper_len: 0,
            long_symbols: Vec::new(),
        }
    }
}

struct BitReader<'a> {
    words: &'a [[u8; 4]],
    next_word_index: usize,
    buffer: u64,
    valid_bits: u32,
}

impl<'a> BitReader<'a> {
    fn new(words: &'a [[u8; 4]]) -> Self {
        let buffer = (u64::from(u32::from_le_bytes(words[0])) << 32)
            | u64::from(u32::from_le_bytes(words[1]));
        Self {
            words,
            next_word_index: 2,
            buffer,
            valid_bits: 64,
        }
    }

    fn peek(&self, bit_count: u32) -> anyhow::Result<u32> {
        if bit_count >= 32 {
            bail!("invalid bit count {bit_count}");
        }
        if bit_count > self.valid_bits {
            bail!("compressed Gw.dat bitstream is truncated");
        }
        Ok(self.peek_padded(bit_count))
    }

    fn peek_padded(&self, bit_count: u32) -> u32 {
        if bit_count == 0 {
            0
        } else {
            (self.buffer >> (64 - bit_count)) as u32
        }
    }

    fn peek_u32(&self) -> u32 {
        (self.buffer >> 32) as u32
    }

    fn advance(&mut self, bit_count: u32) -> anyhow::Result<()> {
        if bit_count == 0 {
            return Ok(());
        }
        if bit_count >= 32 {
            bail!("invalid bit count {bit_count}");
        }
        if bit_count > self.valid_bits {
            bail!("compressed Gw.dat bitstream is truncated");
        }

        self.buffer <<= bit_count;
        self.valid_bits -= bit_count;

        if self.valid_bits <= 32 && self.next_word_index < self.words.len() {
            let word = u64::from(u32::from_le_bytes(self.words[self.next_word_index]));
            self.next_word_index += 1;
            self.buffer |= word << (32 - self.valid_bits);
            self.valid_bits += 32;
        }

        Ok(())
    }

    fn read(&mut self, bit_count: u32) -> anyhow::Result<u32> {
        let value = self.peek(bit_count)?;
        self.advance(bit_count)?;
        Ok(value)
    }
}

struct Decompressor<'a> {
    bits: BitReader<'a>,
    out_size: usize,
}

impl<'a> Decompressor<'a> {
    fn new(input: &'a [u8]) -> anyhow::Result<Self> {
        if input.len() < 12 {
            bail!("compressed Gw.dat payload is too short");
        }
        let (words, remainder) = input.as_chunks::<4>();
        if !remainder.is_empty() {
            bail!("compressed Gw.dat payload length is not 32-bit aligned");
        }

        let (out_size, compressed_words) = words
            .split_last()
            .context("compressed payload is missing output size")?;
        let out_size = u32::from_le_bytes(*out_size) as usize;
        if out_size > MAX_DECOMPRESSED_SIZE {
            bail!(
                "declared Gw.dat decompressed size {out_size} exceeds cap {MAX_DECOMPRESSED_SIZE}"
            );
        }

        let mut bits = BitReader::new(compressed_words);
        bits.advance(4)?;
        Ok(Self { bits, out_size })
    }

    fn decompress(mut self) -> anyhow::Result<Vec<u8>> {
        let mut output = vec![0_u8; self.out_size];
        let mut out = 0_usize;

        let length_bias = self.bits.read(4)?;

        if output.is_empty() {
            return Ok(output);
        }

        while out != output.len() {
            let mut literal_tree = HuffmanData::default();
            self.setup_nodes_and_tree(&mut literal_tree)?;
            let mut distance_tree = HuffmanData::default();
            self.setup_nodes_and_tree(&mut distance_tree)?;

            let block_header = self.bits.read(4)?;
            let mut block_remaining = ((block_header + 1) << 0x0c) as usize;

            while block_remaining > 0 && out != output.len() {
                let symbol = self.decode_symbol(&literal_tree)?;

                if symbol < 0x100 {
                    output[out] = symbol as u8;
                    out += 1;
                } else {
                    let (mut length, extra_len_bits) = length_code(symbol)?;
                    if extra_len_bits != 0 {
                        length |= self.bits.read(extra_len_bits)?;
                    }

                    let copy_len = length_bias as usize + length as usize + 1;
                    let distance_symbol = self.decode_symbol(&distance_tree)?;
                    let (mut backtrack, distance_extra_bits) = distance_code(distance_symbol)?;

                    if distance_extra_bits != 0 {
                        backtrack |= self.bits.read(distance_extra_bits)?;
                    }

                    let distance = backtrack as usize + 1;
                    if distance > out {
                        bail!(
                            "invalid Gw.dat back-reference distance {distance} at output offset {out}"
                        );
                    }
                    if out + copy_len > output.len() {
                        bail!("Gw.dat back-reference exceeds declared output size");
                    }
                    if copy_len <= distance {
                        let source = out - distance;
                        output.copy_within(source..source + copy_len, out);
                        out += copy_len;
                    } else {
                        for _ in 0..copy_len {
                            let byte = output[out - distance];
                            output[out] = byte;
                            out += 1;
                        }
                    }
                }

                block_remaining -= 1;
            }
        }

        Ok(output)
    }

    fn decode_symbol(&mut self, huffman: &HuffmanData) -> anyhow::Result<u32> {
        let current = self.bits.peek_u32();
        let entry = huffman.fast[(current >> 0x18) as usize];
        let mut bit_count = entry.bit_count;
        let mut symbol = entry.symbol;

        if bit_count == INVALID {
            let helper = huffman.helpers[..huffman.helper_len]
                .iter()
                .find(|helper| current >= helper.threshold)
                .context("invalid Huffman helper lookup")?;
            bit_count = helper.bit_count;
            if !(9..32).contains(&bit_count) {
                bail!("invalid Huffman helper bit count {bit_count}");
            }

            let shifted_delta = current.wrapping_sub(helper.threshold) >> (0x20 - bit_count);
            let long_index = helper.last_symbol.wrapping_sub(shifted_delta) as usize;
            symbol = *huffman
                .long_symbols
                .get(long_index)
                .context("Huffman long-symbol lookup out of bounds")?;
        } else if symbol == INVALID {
            bail!("invalid Huffman code prefix");
        }

        if bit_count >= 0x20 {
            bail!("invalid Huffman bit count {bit_count}");
        }
        self.bits.advance(bit_count)?;
        Ok(symbol)
    }

    fn setup_nodes_and_tree(&mut self, huffman: &mut HuffmanData) -> anyhow::Result<()> {
        let symbol_count = self.bits.read(0x10)?;
        let mut next_symbol = vec![0_u32; symbol_count as usize];
        let mut symbol_heads = [INVALID; 0x20];
        let mut total_codes = 0_u32;
        let mut remaining_symbols = symbol_count as usize;

        while remaining_symbols != 0 {
            let current = self.bits.peek_u32();
            let lookup_index = CODE_LENGTH_LOOKUP
                .iter()
                .position(|&(threshold, _)| current >= threshold)
                .context("Huffman setup table lookup failed")?;
            let bit_count = lookup_index as u32 + 3;
            let shift = 0x20 - bit_count;
            let (threshold, table_value) = CODE_LENGTH_LOOKUP[lookup_index];
            let code = table_value.wrapping_sub(current.wrapping_sub(threshold) >> shift);
            let packed = u32::from(
                *CODE_LENGTH_RUNS
                    .get(code as usize)
                    .with_context(|| format!("Huffman setup code {code} is out of range"))?,
            );
            self.bits.advance(bit_count)?;

            let run_len = (packed >> 5) + 1;
            let code_len = (packed & 0x1f) as usize;
            if run_len as usize > remaining_symbols {
                bail!(
                    "Huffman code-length run {run_len} exceeds {remaining_symbols} remaining symbols"
                );
            }

            if code_len != 0 || symbol_count < 2 {
                total_codes += run_len;

                for _ in 0..run_len {
                    remaining_symbols -= 1;
                    let symbol = remaining_symbols as u32;
                    next_symbol[symbol as usize] = symbol_heads[code_len];
                    symbol_heads[code_len] = symbol;
                }
            } else {
                remaining_symbols -= run_len as usize;
            }
        }

        if symbol_count != 0 && total_codes == 0 {
            let last_symbol = symbol_count - 1;
            next_symbol[last_symbol as usize] = symbol_heads[0];
            symbol_heads[0] = last_symbol;
            total_codes = 1;
        }

        *huffman = HuffmanData::default();
        let mut code_len = 0_u32;
        let mut populated = 0_u32;
        let mut code = 0_u32;

        while code_len <= 8 {
            let mut symbol = symbol_heads[code_len as usize];
            if symbol != INVALID {
                let limit = 1_u32 << code_len;
                loop {
                    if code >= limit {
                        bail!("Huffman code {code} exceeds {code_len}-bit range");
                    }
                    if symbol >= symbol_count {
                        bail!("Huffman symbol {symbol} exceeds symbol count {symbol_count}");
                    }

                    let suffix_bits = 8 - code_len;
                    let first = (code << suffix_bits) as usize;
                    let count = 1_usize << suffix_bits;
                    huffman
                        .fast
                        .get_mut(first..first + count)
                        .context("Huffman fast-table range out of bounds")?
                        .fill(HuffmanEntry {
                            bit_count: code_len,
                            symbol,
                        });

                    symbol = next_symbol[symbol as usize];
                    populated += 1;
                    code = code.wrapping_sub(1);
                    if symbol == INVALID {
                        break;
                    }
                }
            }

            code = code.wrapping_mul(2).wrapping_add(1);
            code_len += 1;
        }

        if populated > total_codes {
            bail!("Huffman table over-populated");
        }
        if populated == total_codes {
            return Ok(());
        }

        huffman.long_symbols = vec![INVALID; (total_codes - populated) as usize];
        let mut long_symbol_count = 0_usize;

        while code_len <= 0x1f {
            let mut symbol = symbol_heads[code_len as usize];
            if symbol != INVALID {
                let limit = 1_u32 << code_len;
                loop {
                    if code >= limit {
                        bail!("Huffman code {code} exceeds {code_len}-bit range");
                    }
                    if symbol >= symbol_count {
                        bail!("Huffman symbol {symbol} exceeds symbol count {symbol_count}");
                    }

                    let fast_index = (code >> (code_len - 8)) as usize;
                    let fast_entry = huffman
                        .fast
                        .get_mut(fast_index)
                        .context("Huffman long-code prefix out of bounds")?;
                    if fast_entry.symbol != INVALID {
                        bail!("Huffman long code overlaps a short code");
                    }
                    fast_entry.bit_count = INVALID;

                    let long_symbol = huffman
                        .long_symbols
                        .get_mut(long_symbol_count)
                        .context("Huffman long-symbol table overflow")?;
                    *long_symbol = symbol;
                    long_symbol_count += 1;
                    symbol = next_symbol[symbol as usize];
                    code = code.wrapping_sub(1);
                    if symbol == INVALID {
                        break;
                    }
                }

                let helper = huffman
                    .helpers
                    .get_mut(huffman.helper_len)
                    .context("Huffman helper table overflow")?;
                *helper = HuffmanHelper {
                    threshold: code.wrapping_add(1).wrapping_shl(0x20 - code_len),
                    last_symbol: (long_symbol_count - 1) as u32,
                    bit_count: code_len,
                };
                huffman.helper_len += 1;
            }

            code = code.wrapping_mul(2).wrapping_add(1);
            code_len += 1;
        }

        if long_symbol_count != huffman.long_symbols.len() {
            bail!("Huffman long-symbol table is incomplete");
        }

        Ok(())
    }
}

fn length_code(symbol: u32) -> anyhow::Result<(u32, u32)> {
    let index = symbol
        .checked_sub(0x100)
        .context("literal symbol used as a length code")? as usize;
    let Some((&base, &extra_bits)) = LENGTH_BASES.get(index).zip(LENGTH_EXTRA_BITS.get(index))
    else {
        bail!("length symbol {symbol} is out of range");
    };
    Ok((u32::from(base), u32::from(extra_bits)))
}

fn distance_code(symbol: u32) -> anyhow::Result<(u32, u32)> {
    let index = symbol as usize;
    let Some((&base, &extra_bits)) = DISTANCE_BASES
        .get(index)
        .zip(DISTANCE_EXTRA_BITS.get(index))
    else {
        bail!("distance symbol {symbol} is out of range");
    };
    Ok((u32::from(base), u32::from(extra_bits)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPEATED_VECTOR: &str = "\
        061d01020821f4e421841042841042081042082121f44c8082104208099e444f\
        a0004083ffa41a98ffffffff00a209fe0800018002180000";

    #[test]
    fn rejects_too_short_payload() {
        let err = decompress_gw_dat(&[0; 8]).expect_err("short payload must fail");
        assert!(err.to_string().contains("too short"));
    }

    #[test]
    fn rejects_non_aligned_payload() {
        let err = decompress_gw_dat(&[0; 13]).expect_err("unaligned payload must fail");
        assert!(err.to_string().contains("not 32-bit aligned"));
    }

    #[test]
    fn accepts_zero_declared_output_size() -> anyhow::Result<()> {
        let payload = [0_u8; 12];
        let out = decompress_gw_dat(&payload)?;
        assert!(out.is_empty());
        Ok(())
    }

    #[test]
    fn decompresses_repeated_back_reference_vector() -> anyhow::Result<()> {
        // Local first-party Gw.dat MFT entry 8096.
        let input = hex::decode(REPEATED_VECTOR)?;
        let mut expected = [0x06, 0, 0, 0, 0x10, 0].repeat(1024);
        expected.extend_from_slice(&[0, 0x54]);

        assert_eq!(decompress_gw_dat(&input)?, expected);
        Ok(())
    }

    #[test]
    fn rejects_truncated_bitstream() -> anyhow::Result<()> {
        let input = hex::decode(REPEATED_VECTOR)?;
        let mut truncated = input[..input.len() - 12].to_vec();
        truncated.extend_from_slice(&input[input.len() - 4..]);

        let err = decompress_gw_dat(&truncated).expect_err("truncated bitstream must fail");
        assert!(err.to_string().contains("bitstream is truncated"));
        Ok(())
    }

    #[test]
    fn rejects_invalid_huffman_data() -> anyhow::Result<()> {
        for (byte, mask, expected) in [
            (0, 1 << 1, "code-length run"),
            (6, 1 << 4, "invalid Huffman code prefix"),
        ] {
            let mut input = hex::decode(REPEATED_VECTOR)?;
            input[byte] ^= mask;
            let err = decompress_gw_dat(&input).expect_err("invalid Huffman data must fail");
            assert!(err.to_string().contains(expected), "{err:#}");
        }
        Ok(())
    }

    #[test]
    fn decompresses_texture_header_vector() -> anyhow::Result<()> {
        // Local first-party Gw.dat MFT entry 63319.
        let input = hex::decode(
            "380e010299c0792a841042c8082107211072104240c84984404ff25420a7693c2\
             1a782d39c04a1073c159e090607001a116c30c12abc7306e2fdd779317da190\
             58608ca50800018054000000",
        )?;
        let expected = hex::decode(
            "4154455844585431200020000c0000000100000000008f6d0c00000001000000\
             000000060c00000001000000000000360c00000001000000000000c00c000000\
             01000000000000c00c00000001000000000000c0",
        )?;

        assert_eq!(decompress_gw_dat(&input)?, expected);
        Ok(())
    }
}
