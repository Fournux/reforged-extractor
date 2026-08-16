mod decompress;
mod dxt;

use decompress::decompress_atex_blocks;
use dxt::{decode_dxt1_rgba, decode_dxt3_rgba, decode_dxt5_rgba, decode_dxta_rgba};

use crate::io_util::read_u32;
use anyhow::{Context, bail};
use image::{ColorType, ImageFormat, save_buffer_with_format};
use std::path::Path;

const ATEX_HEADER_LEN: usize = 12;
pub(crate) const ATEX_ENCODED_HEADER_LEN: usize = 20;
const DDS_HEADER_LEN: usize = 128;
const DDS_HEADER_SIZE: u32 = 124;
const DDPF_RGB: u32 = 0x40;
const DDPF_BUMPDUDV: u32 = 0x0008_0000;
const DXT_BLOCK_SIDE: usize = 4;
const RGBA_BYTES_PER_PIXEL: usize = 4;
const MAX_TEXTURE_PIXELS: usize = 8192 * 8192;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AtexContainer {
    Atex,
    Attx,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AtexDxtFormat {
    Dxt1,
    Dxt2,
    Dxt3,
    Dxt4,
    Dxt5,
    DxtN,
    DxtA,
    DxtL,
}

impl AtexDxtFormat {
    pub(crate) fn from_fourcc(fourcc: [u8; 4]) -> anyhow::Result<Self> {
        match &fourcc {
            b"DXT1" => Ok(Self::Dxt1),
            b"DXT2" => Ok(Self::Dxt2),
            b"DXT3" => Ok(Self::Dxt3),
            b"DXT4" => Ok(Self::Dxt4),
            b"DXT5" => Ok(Self::Dxt5),
            b"DXTN" => Ok(Self::DxtN),
            b"DXTA" => Ok(Self::DxtA),
            b"DXTL" => Ok(Self::DxtL),
            _ => bail!(
                "unsupported ATEX DXT FourCC {}",
                String::from_utf8_lossy(&fourcc)
            ),
        }
    }

    pub(crate) fn as_fourcc(self) -> &'static str {
        match self {
            Self::Dxt1 => "DXT1",
            Self::Dxt2 => "DXT2",
            Self::Dxt3 => "DXT3",
            Self::Dxt4 => "DXT4",
            Self::Dxt5 => "DXT5",
            Self::DxtN => "DXTN",
            Self::DxtA => "DXTA",
            Self::DxtL => "DXTL",
        }
    }

    fn block_layout(self) -> (usize, usize) {
        match self {
            Self::Dxt1 => (0, 2),
            Self::DxtA => (2, 0),
            _ => (2, 2),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AtexHeader {
    pub(crate) container: AtexContainer,
    pub(crate) format: AtexDxtFormat,
    pub(crate) width: u16,
    pub(crate) height: u16,
}

pub(crate) fn parse_header(bytes: &[u8]) -> anyhow::Result<AtexHeader> {
    let header = bytes
        .get(..ATEX_HEADER_LEN)
        .context("ATEX header shorter than 12 bytes")?;

    let container = match &header[0..4] {
        b"ATEX" => AtexContainer::Atex,
        b"ATTX" => AtexContainer::Attx,
        other => bail!(
            "not an ATEX/ATTX texture: {}",
            String::from_utf8_lossy(other)
        ),
    };

    let format = AtexDxtFormat::from_fourcc([header[4], header[5], header[6], header[7]])?;
    let width = u16::from_le_bytes([header[8], header[9]]);
    let height = u16::from_le_bytes([header[10], header[11]]);
    if width == 0 || height == 0 {
        bail!("ATEX texture has zero dimensions: {width}x{height}");
    }

    Ok(AtexHeader {
        container,
        format,
        width,
        height,
    })
}

pub(crate) fn detect_kind(bytes: &[u8]) -> Option<&'static str> {
    let header = parse_header(bytes).ok()?;
    match header.container {
        AtexContainer::Atex => Some("atex_texture"),
        AtexContainer::Attx => Some("attx_texture"),
    }
}

pub(crate) fn save_atex_as_png(atex_bytes: &[u8], out_path: &Path) -> anyhow::Result<()> {
    save_atex_as_png_with_alpha_policy(atex_bytes, out_path, AlphaPolicy::ForceOpaque)
}

pub(crate) fn save_atex_as_png_preserve_alpha(
    atex_bytes: &[u8],
    out_path: &Path,
) -> anyhow::Result<()> {
    save_atex_as_png_with_alpha_policy(atex_bytes, out_path, AlphaPolicy::Preserve)
}

pub(crate) fn save_rgba_as_png(
    width: u32,
    height: u32,
    rgba: &[u8],
    out_path: &Path,
) -> anyhow::Result<()> {
    let expected_len = checked_rgba_len(width as usize, height as usize, "RGBA")?;
    if rgba.len() != expected_len {
        bail!(
            "RGBA buffer length {} does not match dimensions {}x{}",
            rgba.len(),
            width,
            height
        );
    }
    save_buffer_with_format(
        out_path,
        rgba,
        width,
        height,
        ColorType::Rgba8,
        ImageFormat::Png,
    )
    .with_context(|| format!("writing PNG {}", out_path.display()))?;
    Ok(())
}

pub(crate) fn decode_dds_rgba(dds_bytes: &[u8]) -> anyhow::Result<(u32, u32, Vec<u8>, String)> {
    let magic = dds_bytes
        .get(..4)
        .context("DDS payload shorter than magic")?;
    if magic != b"DDS " {
        bail!("not a DDS texture");
    }
    let header_size = read_u32(dds_bytes, 4).context("DDS missing header size")?;
    if header_size != DDS_HEADER_SIZE {
        bail!("unsupported DDS header size {header_size}");
    }
    let height = read_u32(dds_bytes, 12).context("DDS missing height")?;
    let width = read_u32(dds_bytes, 16).context("DDS missing width")?;
    if width == 0 || height == 0 {
        bail!("unsupported DDS dimensions {width}x{height}");
    }
    let pf_flags = read_u32(dds_bytes, 80).context("DDS missing pixel-format flags")?;
    let fourcc = dds_bytes
        .get(84..88)
        .context("DDS missing pixel-format FourCC")?;
    let rgb_bit_count = read_u32(dds_bytes, 88).context("DDS missing RGB bit count")?;
    let masks = [
        read_u32(dds_bytes, 92).context("DDS missing R mask")?,
        read_u32(dds_bytes, 96).context("DDS missing G mask")?,
        read_u32(dds_bytes, 100).context("DDS missing B mask")?,
        read_u32(dds_bytes, 104).context("DDS missing A mask")?,
    ];
    let payload = dds_bytes
        .get(DDS_HEADER_LEN..)
        .context("DDS missing texture payload")?;
    let (rgba, format) = match fourcc {
        b"DXT1" => (
            decode_dxt1_rgba(payload, width as usize, height as usize)?,
            "DXT1".to_string(),
        ),
        b"DXT3" => (
            decode_dxt3_rgba(payload, width as usize, height as usize)?,
            "DXT3".to_string(),
        ),
        b"DXT5" => (
            decode_dxt5_rgba(payload, width as usize, height as usize, false)?,
            "DXT5".to_string(),
        ),
        b"\0\0\0\0" if pf_flags & DDPF_RGB != 0 => (
            decode_uncompressed_dds_rgba(payload, width, height, rgb_bit_count, masks)?,
            format!("RGB{rgb_bit_count}"),
        ),
        b"\0\0\0\0" if pf_flags & DDPF_BUMPDUDV != 0 => (
            decode_uncompressed_dds_rgba(payload, width, height, rgb_bit_count, masks)?,
            format!("BUMP{rgb_bit_count}"),
        ),
        other => bail!(
            "unsupported DDS FourCC {} flags=0x{pf_flags:x} rgb_bits={rgb_bit_count}",
            String::from_utf8_lossy(other)
        ),
    };
    Ok((width, height, rgba, format))
}

fn decode_uncompressed_dds_rgba(
    payload: &[u8],
    width: u32,
    height: u32,
    rgb_bit_count: u32,
    masks: [u32; 4],
) -> anyhow::Result<Vec<u8>> {
    if !(16..=32).contains(&rgb_bit_count) || !rgb_bit_count.is_multiple_of(8) {
        bail!("unsupported uncompressed DDS bit depth {rgb_bit_count}");
    }
    let bytes_per_pixel = (rgb_bit_count / 8) as usize;
    let width = usize::try_from(width).context("DDS width does not fit usize")?;
    let height = usize::try_from(height).context("DDS height does not fit usize")?;
    let pixel_count = checked_pixel_count(width, height, "uncompressed DDS")?;
    let expected = pixel_count
        .checked_mul(bytes_per_pixel)
        .context("DDS dimensions overflow")?;
    if payload.len() < expected {
        bail!(
            "uncompressed DDS payload underrun: have {}, need {expected}",
            payload.len()
        );
    }

    let mut rgba = vec![0_u8; checked_rgba_len(width, height, "uncompressed DDS")?];
    for i in 0..pixel_count {
        let src = i * bytes_per_pixel;
        let raw = match bytes_per_pixel {
            2 => u32::from(u16::from_le_bytes([payload[src], payload[src + 1]])),
            3 => {
                u32::from(payload[src])
                    | (u32::from(payload[src + 1]) << 8)
                    | (u32::from(payload[src + 2]) << 16)
            }
            4 => u32::from_le_bytes([
                payload[src],
                payload[src + 1],
                payload[src + 2],
                payload[src + 3],
            ]),
            _ => unreachable!(),
        };
        let dst = i * RGBA_BYTES_PER_PIXEL;
        rgba[dst] = extract_masked_channel(raw, masks[0]);
        rgba[dst + 1] = extract_masked_channel(raw, masks[1]);
        rgba[dst + 2] = extract_masked_channel(raw, masks[2]);
        rgba[dst + 3] = if masks[3] == 0 {
            255
        } else {
            extract_masked_channel(raw, masks[3])
        };
    }
    Ok(rgba)
}

fn extract_masked_channel(raw: u32, mask: u32) -> u8 {
    if mask == 0 {
        return 0;
    }
    let shift = mask.trailing_zeros();
    let max = u64::from(mask >> shift);
    let value = u64::from((raw & mask) >> shift);
    ((value * 255 + max / 2) / max) as u8
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AlphaPolicy {
    Preserve,
    ForceOpaque,
}

fn save_atex_as_png_with_alpha_policy(
    atex_bytes: &[u8],
    out_path: &Path,
    alpha_policy: AlphaPolicy,
) -> anyhow::Result<()> {
    let (width, height, mut rgba) = decode_atex_rgba(atex_bytes)?;
    if alpha_policy == AlphaPolicy::ForceOpaque {
        // Skill icons use client-specific alpha/luminance data; ordinary PNG viewers
        // display that as false transparency, so skill export keeps the existing
        // opaque policy. Inventory-icon investigations can preserve alpha separately.
        for pixel in rgba.chunks_exact_mut(RGBA_BYTES_PER_PIXEL) {
            pixel[3] = 255;
        }
    }
    save_rgba_as_png(width, height, &rgba, out_path)
}

pub(crate) fn decode_atex_rgba(atex_bytes: &[u8]) -> anyhow::Result<(u32, u32, Vec<u8>)> {
    let header = parse_header(atex_bytes)?;
    let dxt = decompress_atex_blocks(atex_bytes, header)?;
    let rgba = match header.format {
        AtexDxtFormat::Dxt1 => {
            decode_dxt1_rgba(&dxt, header.width as usize, header.height as usize)?
        }
        AtexDxtFormat::Dxt2 | AtexDxtFormat::Dxt3 | AtexDxtFormat::DxtN => {
            decode_dxt3_rgba(&dxt, header.width as usize, header.height as usize)?
        }
        AtexDxtFormat::DxtA => {
            decode_dxta_rgba(&dxt, header.width as usize, header.height as usize)?
        }
        AtexDxtFormat::Dxt4 | AtexDxtFormat::Dxt5 | AtexDxtFormat::DxtL => {
            let premultiply = header.format == AtexDxtFormat::DxtL;
            decode_dxt5_rgba(
                &dxt,
                header.width as usize,
                header.height as usize,
                premultiply,
            )?
        }
    };
    Ok((header.width as u32, header.height as u32, rgba))
}

fn checked_pixel_count(width: usize, height: usize, context: &str) -> anyhow::Result<usize> {
    let pixels = width
        .checked_mul(height)
        .with_context(|| format!("{context} pixel count overflow"))?;
    if pixels > MAX_TEXTURE_PIXELS {
        bail!("{context} texture too large: {width}x{height}");
    }
    Ok(pixels)
}

fn checked_rgba_len(width: usize, height: usize, context: &str) -> anyhow::Result<usize> {
    checked_pixel_count(width, height, context)?
        .checked_mul(RGBA_BYTES_PER_PIXEL)
        .with_context(|| format!("{context} RGBA byte count overflow"))
}

fn dxt_block_count(width: usize, height: usize, context: &str) -> anyhow::Result<usize> {
    checked_pixel_count(width, height, context)?;
    let blocks_x = width.div_ceil(DXT_BLOCK_SIDE);
    let blocks_y = height.div_ceil(DXT_BLOCK_SIDE);
    blocks_x
        .checked_mul(blocks_y)
        .with_context(|| format!("{context} DXT block count overflow"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uncompressed_dds(
        width: u32,
        height: u32,
        flags: u32,
        bit_count: u32,
        masks: [u32; 4],
        payload: &[u8],
    ) -> Vec<u8> {
        let mut dds = vec![0_u8; DDS_HEADER_LEN];
        dds[..4].copy_from_slice(b"DDS ");
        dds[4..8].copy_from_slice(&DDS_HEADER_SIZE.to_le_bytes());
        dds[12..16].copy_from_slice(&height.to_le_bytes());
        dds[16..20].copy_from_slice(&width.to_le_bytes());
        dds[80..84].copy_from_slice(&flags.to_le_bytes());
        dds[88..92].copy_from_slice(&bit_count.to_le_bytes());
        for (offset, mask) in [92, 96, 100, 104].into_iter().zip(masks) {
            dds[offset..offset + 4].copy_from_slice(&mask.to_le_bytes());
        }
        dds.extend_from_slice(payload);
        dds
    }

    #[test]
    fn uncompressed_dds_rejects_oversized_dimensions_before_payload_len() {
        let dds = uncompressed_dds(8193, 8193, DDPF_RGB, 32, [0; 4], &[]);

        let err = decode_dds_rgba(&dds).expect_err("oversized DDS must fail before allocation");

        assert!(err.to_string().contains("texture too large"));
    }

    #[test]
    fn uncompressed_dds_rejects_non_byte_aligned_depth() {
        let dds = uncompressed_dds(1, 1, DDPF_RGB, 17, [0xffff, 0, 0, 0], &[0, 0]);

        let err = decode_dds_rgba(&dds).expect_err("17-bit DDS must be rejected");

        assert!(
            err.to_string()
                .contains("unsupported uncompressed DDS bit depth 17")
        );
    }

    #[test]
    fn masked_channel_scaling_handles_a_full_width_mask() {
        assert_eq!(extract_masked_channel(u32::MAX, u32::MAX), 255);
    }

    #[test]
    fn dxta_bc4_is_exported_as_opaque_grayscale() {
        let rgba = decode_dxta_rgba(&[10, 20, 0, 0, 0, 0, 0, 0], 4, 4).unwrap();

        assert!(rgba.chunks_exact(4).all(|pixel| pixel == [10, 10, 10, 255]));
    }

    #[test]
    fn dds_bump16_preserves_both_stored_channels() {
        let dds = uncompressed_dds(1, 1, DDPF_BUMPDUDV, 16, [0xff, 0xff00, 0, 0], &[10, 20]);

        let (width, height, rgba, format) = decode_dds_rgba(&dds).unwrap();

        assert_eq!((width, height), (1, 1));
        assert_eq!(rgba, [10, 20, 0, 255]);
        assert_eq!(format, "BUMP16");
    }
}
