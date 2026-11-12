// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The icon of a Windows program, as a PNG file for its launcher entry.
//!
//! A program's icon is a group of images in its resources. The first group
//! is the one a file manager shows. Its largest, deepest image is taken:
//! when that image is already a PNG it is used as it is, and otherwise it is
//! a device-independent bitmap with a transparency mask, which is decoded
//! and written out as a PNG.
//!
//! The program file comes out of an installer and is not trusted. Sizes are
//! capped, every index is checked, and nothing is decompressed: a PNG image
//! is copied, never decoded, and the PNG written here stores its pixels
//! without compression, so there is no compressor to get wrong.

use std::path::Path;

use crate::arch::Endianness;
use crate::error::InspectError;
use crate::pe_res::{self, RT_GROUP_ICON, RT_ICON};
use crate::reader::{u16_at, u32_at};

const LITTLE: Endianness = Endianness::Little;
const BIG: Endianness = Endianness::Big;

/// The first bytes of every PNG file.
const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
/// Bytes of an icon group read.
const MAX_GROUP_BYTES: usize = 64 * 1024;
/// Images considered in one icon group.
const MAX_GROUP_ENTRIES: usize = 64;
/// Bytes of one icon image read.
const MAX_ICON_BYTES: usize = 2 * 1024 * 1024;
/// The largest width or height of a bitmap icon. The format's own limit.
const MAX_BITMAP_SIDE: u32 = 256;
/// The largest width or height accepted for an icon that is a PNG.
const MAX_PNG_SIDE: u32 = 1024;
/// A palette entry used when an index points outside the palette.
const BLACK: &[u8] = &[0, 0, 0, 0];

/// A program's icon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedIcon {
    /// The icon as a PNG file.
    pub png: Vec<u8>,
    /// Its width in pixels.
    pub width: u32,
    /// Its height in pixels.
    pub height: u32,
    /// The colour depth of the image it was taken from, in bits per pixel.
    pub source_bits: u16,
    /// Whether the image was already a PNG in the program file.
    pub was_png: bool,
}

struct GroupEntry {
    width: u32,
    bits: u16,
    id: u16,
}

/// The largest image of an icon group and, among equals, the deepest.
fn best_entry(group: &[u8]) -> Option<GroupEntry> {
    let count = usize::from(u16_at(group, 4, LITTLE)?).min(MAX_GROUP_ENTRIES);
    (0..count)
        .filter_map(|index| {
            let entry = group.get(6 + index * 14..6 + index * 14 + 14)?;
            // A width of zero stands for 256.
            let width = match *entry.first()? {
                0 => 256,
                width => u32::from(width),
            };
            Some(GroupEntry {
                width,
                bits: u16_at(entry, 6, LITTLE)?,
                id: u16_at(entry, 12, LITTLE)?,
            })
        })
        .max_by_key(|entry| (entry.width, entry.bits))
}

/// The icon of the PE image at `path`. `Ok(None)` when the file is not a PE
/// image, has no icon, or has one in a form that is not decoded.
///
/// # Errors
///
/// [`InspectError`] when the file cannot be read, its resource tree is cut
/// short, or a read limit is hit.
pub fn extract_icon(path: &Path) -> Result<Option<ExtractedIcon>, InspectError> {
    let mut reader = pe_res::open(path)?;
    let Some(layout) = pe_res::layout(&mut reader)? else {
        return Ok(None);
    };
    let Some(group) = pe_res::resource(&mut reader, &layout, RT_GROUP_ICON, None, MAX_GROUP_BYTES)?
    else {
        return Ok(None);
    };
    let Some(best) = best_entry(&group) else {
        return Ok(None);
    };
    let image = pe_res::resource(
        &mut reader,
        &layout,
        RT_ICON,
        Some(u32::from(best.id)),
        MAX_ICON_BYTES,
    )?;
    Ok(image.and_then(|image| decode(&image)))
}

/// Turns one icon image into a PNG.
fn decode(image: &[u8]) -> Option<ExtractedIcon> {
    if image.starts_with(&PNG_MAGIC) {
        // The first chunk of a PNG is its header: width and height.
        if image.get(12..16)? != b"IHDR" {
            return None;
        }
        let width = u32_at(image, 16, BIG)?;
        let height = u32_at(image, 20, BIG)?;
        if width == 0 || height == 0 || width > MAX_PNG_SIDE || height > MAX_PNG_SIDE {
            return None;
        }
        return Some(ExtractedIcon {
            png: image.to_vec(),
            width,
            height,
            source_bits: 32,
            was_png: true,
        });
    }
    let bitmap = decode_bitmap(image)?;
    Some(ExtractedIcon {
        png: encode_png(&bitmap.rgba, bitmap.width, bitmap.height),
        width: bitmap.width,
        height: bitmap.height,
        source_bits: bitmap.bits,
        was_png: false,
    })
}

struct Bitmap {
    /// Red, green, blue and alpha for every pixel, top row first.
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    bits: u16,
}

/// Decodes an icon's device-independent bitmap: a header, a palette for
/// depths of eight bits and below, the colour rows from the bottom up, each
/// padded to four bytes, and a one-bit transparency mask laid out the same
/// way.
fn decode_bitmap(image: &[u8]) -> Option<Bitmap> {
    let header_len = usize::try_from(u32_at(image, 0, LITTLE)?).ok()?;
    let width = u32_at(image, 4, LITTLE)?;
    // The stored height counts the colour rows and the mask rows.
    let doubled = u32_at(image, 8, LITTLE)?;
    let bits = u16_at(image, 14, LITTLE)?;
    let compression = u32_at(image, 16, LITTLE)?;
    let colours_used = usize::try_from(u32_at(image, 32, LITTLE)?).ok()?;
    let height = doubled / 2;
    let plausible = (40..=124).contains(&header_len)
        && compression == 0
        && width != 0
        && height != 0
        && width <= MAX_BITMAP_SIDE
        && height <= MAX_BITMAP_SIDE
        && doubled.is_multiple_of(2)
        && matches!(bits, 1 | 4 | 8 | 24 | 32);
    if !plausible {
        return None;
    }
    let columns = usize::try_from(width).ok()?;
    let rows = usize::try_from(height).ok()?;
    let depth = usize::from(bits);

    let palette_len = if bits <= 8 {
        let full = 1usize << depth;
        if colours_used == 0 {
            full
        } else {
            colours_used.min(full)
        }
    } else {
        0
    };
    let pixels_at = header_len + palette_len * 4;
    let row_len = (columns * depth).div_ceil(32) * 4;
    let mask_row_len = columns.div_ceil(32) * 4;
    let mask_at = pixels_at + row_len * rows;
    let palette = image.get(header_len..pixels_at)?;
    let pixels = image.get(pixels_at..mask_at)?;
    // Some writers leave the mask out; every pixel is then opaque.
    let mask = image.get(mask_at..mask_at + mask_row_len * rows);

    let mut rgba = vec![0u8; columns * rows * 4];
    let mut masked = vec![false; columns * rows];
    let mut any_alpha = false;
    for y in 0..rows {
        let stored = rows - 1 - y;
        let row = pixels.get(stored * row_len..(stored + 1) * row_len)?;
        let mask_row =
            mask.and_then(|mask| mask.get(stored * mask_row_len..(stored + 1) * mask_row_len));
        for x in 0..columns {
            let (blue, green, red, alpha) = match bits {
                32 => {
                    let pixel = row.get(x * 4..x * 4 + 4)?;
                    (pixel[0], pixel[1], pixel[2], pixel[3])
                }
                24 => {
                    let pixel = row.get(x * 3..x * 3 + 3)?;
                    (pixel[0], pixel[1], pixel[2], 0)
                }
                _ => {
                    let index = match bits {
                        8 => usize::from(*row.get(x)?),
                        4 => {
                            let byte = *row.get(x / 2)?;
                            usize::from(if x.is_multiple_of(2) {
                                byte >> 4
                            } else {
                                byte & 0x0f
                            })
                        }
                        _ => usize::from((*row.get(x / 8)? >> (7 - x % 8)) & 1),
                    };
                    let entry = palette.get(index * 4..index * 4 + 4).unwrap_or(BLACK);
                    (entry[0], entry[1], entry[2], 0)
                }
            };
            let transparent = mask_row
                .and_then(|mask_row| mask_row.get(x / 8))
                .is_some_and(|byte| (byte >> (7 - x % 8)) & 1 == 1);
            any_alpha |= alpha != 0;
            let at = y * columns + x;
            if let Some(flag) = masked.get_mut(at) {
                *flag = transparent;
            }
            let out = rgba.get_mut(at * 4..at * 4 + 4)?;
            out[0] = red;
            out[1] = green;
            out[2] = blue;
            out[3] = if bits == 32 {
                alpha
            } else if transparent {
                0
            } else {
                255
            };
        }
    }
    // A 32-bit image whose alpha channel is all zero is an image without
    // one: its transparency is in the mask.
    if bits == 32 && !any_alpha {
        for (pixel, transparent) in rgba.as_chunks_mut::<4>().0.iter_mut().zip(&masked) {
            pixel[3] = if *transparent { 0 } else { 255 };
        }
    }
    Some(Bitmap {
        rgba,
        width,
        height,
        bits,
    })
}

/// The CRC-32 of the PNG and zip formats.
fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// The Adler-32 checksum that ends a zlib stream.
fn adler32(data: &[u8]) -> u32 {
    let mut low = 1u32;
    let mut high = 0u32;
    for &byte in data {
        low = (low + u32::from(byte)) % 65_521;
        high = (high + low) % 65_521;
    }
    (high << 16) | low
}

fn push_chunk(png: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    let length = u32::try_from(data.len()).unwrap_or(u32::MAX);
    png.extend_from_slice(&length.to_be_bytes());
    let mut body = kind.to_vec();
    body.extend_from_slice(data);
    png.extend_from_slice(&body);
    png.extend_from_slice(&crc32(&body).to_be_bytes());
}

/// Writes pixels as a PNG: eight bits per channel, with alpha, each row
/// unfiltered, the data in stored (uncompressed) deflate blocks.
fn encode_png(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    let row_len = usize::try_from(width).unwrap_or(0) * 4;
    let mut raw = Vec::with_capacity(rgba.len() + usize::try_from(height).unwrap_or(0));
    if row_len > 0 {
        for row in rgba.chunks_exact(row_len) {
            // Filter type 0: the row as it is.
            raw.push(0);
            raw.extend_from_slice(row);
        }
    }
    // A zlib stream: header, stored blocks of at most 65 535 bytes, checksum.
    let mut stream = vec![0x78, 0x01];
    let mut blocks = raw.chunks(65_535).peekable();
    while let Some(block) = blocks.next() {
        let length = u16::try_from(block.len()).unwrap_or(u16::MAX);
        stream.push(u8::from(blocks.peek().is_none()));
        stream.extend_from_slice(&length.to_le_bytes());
        stream.extend_from_slice(&(!length).to_le_bytes());
        stream.extend_from_slice(block);
    }
    stream.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    // Bit depth 8, colour type 6 (red, green, blue, alpha), no interlace.
    header.extend_from_slice(&[8, 6, 0, 0, 0]);

    let mut png = PNG_MAGIC.to_vec();
    push_chunk(&mut png, b"IHDR", &header);
    push_chunk(&mut png, b"IDAT", &stream);
    push_chunk(&mut png, b"IEND", &[]);
    png
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::pe_res::tests::image;
    use crate::testdata::TempDir;

    /// The pixels of a PNG written by [`encode_png`], read back: width,
    /// height and the RGBA bytes. Checks every chunk's CRC and the stream's
    /// checksum on the way.
    pub(crate) fn read_png(png: &[u8]) -> (u32, u32, Vec<u8>) {
        assert!(png.starts_with(&PNG_MAGIC));
        let mut at = 8;
        let mut size = (0, 0);
        let mut stream = Vec::new();
        let mut ended = false;
        while at < png.len() {
            let length = usize::try_from(u32_at(png, at, BIG).unwrap()).unwrap();
            let body = &png[at + 4..at + 8 + length];
            let stored = u32_at(png, at + 8 + length, BIG).unwrap();
            assert_eq!(crc32(body), stored, "chunk CRC");
            let data = &body[4..];
            match &body[..4] {
                b"IHDR" => {
                    size = (u32_at(data, 0, BIG).unwrap(), u32_at(data, 4, BIG).unwrap());
                    assert_eq!(&data[8..], [8, 6, 0, 0, 0]);
                }
                b"IDAT" => stream.extend_from_slice(data),
                b"IEND" => ended = true,
                other => panic!("unexpected chunk {other:?}"),
            }
            at += 12 + length;
        }
        assert!(ended);
        assert_eq!(&stream[..2], [0x78, 0x01]);
        let mut raw = Vec::new();
        let mut at = 2;
        loop {
            let last = stream[at];
            let length = usize::from(u16_at(&stream, at + 1, LITTLE).unwrap());
            let inverse = u16_at(&stream, at + 3, LITTLE).unwrap();
            assert_eq!(usize::from(!inverse), length);
            raw.extend_from_slice(&stream[at + 5..at + 5 + length]);
            at += 5 + length;
            if last == 1 {
                break;
            }
            assert_eq!(last, 0);
        }
        assert_eq!(u32_at(&stream, at, BIG).unwrap(), adler32(&raw));
        assert_eq!(at + 4, stream.len());
        let (width, height) = size;
        let row_len = usize::try_from(width).unwrap() * 4 + 1;
        assert_eq!(raw.len(), row_len * usize::try_from(height).unwrap());
        let mut rgba = Vec::new();
        for row in raw.chunks_exact(row_len) {
            assert_eq!(row[0], 0, "filter type");
            rgba.extend_from_slice(&row[1..]);
        }
        (width, height, rgba)
    }

    /// An icon bitmap: `rows` are given top row first, as palette indexes or
    /// as BGRA values, and `transparent` marks masked pixels.
    pub(crate) fn bitmap(
        bits: u16,
        width: usize,
        palette: &[[u8; 3]],
        rows: &[Vec<u32>],
        transparent: &[(usize, usize)],
    ) -> Vec<u8> {
        let height = rows.len();
        let mut out = Vec::new();
        out.extend_from_slice(&40u32.to_le_bytes());
        out.extend_from_slice(&u32::try_from(width).unwrap().to_le_bytes());
        out.extend_from_slice(&u32::try_from(height * 2).unwrap().to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&bits.to_le_bytes());
        // No compression; then the image size and resolution, left at zero;
        // then the number of palette entries that follow.
        out.extend_from_slice(&[0u8; 16]);
        out.extend_from_slice(&u32::try_from(palette.len()).unwrap().to_le_bytes());
        out.extend_from_slice(&[0u8; 4]);
        for [red, green, blue] in palette {
            out.extend_from_slice(&[*blue, *green, *red, 0]);
        }
        let row_len = (width * usize::from(bits)).div_ceil(32) * 4;
        for row in rows.iter().rev() {
            let mut bytes = vec![0u8; row_len];
            for (x, value) in row.iter().enumerate() {
                match bits {
                    32 => bytes[x * 4..x * 4 + 4].copy_from_slice(&value.to_le_bytes()),
                    24 => bytes[x * 3..x * 3 + 3].copy_from_slice(&value.to_le_bytes()[..3]),
                    8 => bytes[x] = u8::try_from(*value).unwrap(),
                    4 => bytes[x / 2] |= u8::try_from(*value).unwrap() << (4 * (1 - x % 2)),
                    _ => bytes[x / 8] |= u8::try_from(*value).unwrap() << (7 - x % 8),
                }
            }
            out.extend_from_slice(&bytes);
        }
        let mask_row_len = width.div_ceil(32) * 4;
        for y in (0..height).rev() {
            let mut bytes = vec![0u8; mask_row_len];
            for (x, masked_y) in transparent {
                if *masked_y == y {
                    bytes[x / 8] |= 1 << (7 - x % 8);
                }
            }
            out.extend_from_slice(&bytes);
        }
        out
    }

    /// An icon group naming images by identifier: `(width, bits, id)`.
    pub(crate) fn group(entries: &[(u8, u16, u16)]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&u16::try_from(entries.len()).unwrap().to_le_bytes());
        for (width, bits, id) in entries {
            out.extend_from_slice(&[*width, *width, 0, 0]);
            out.extend_from_slice(&1u16.to_le_bytes());
            out.extend_from_slice(&bits.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&id.to_le_bytes());
        }
        out
    }

    const RED: [u8; 4] = [255, 0, 0, 255];
    const GREEN: [u8; 4] = [0, 255, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];
    const CLEAR_BLACK: [u8; 4] = [0, 0, 0, 0];

    #[test]
    fn the_checksums_match_their_published_check_values() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        assert_eq!(crc32(b""), 0);
        assert_eq!(adler32(b"Wikipedia"), 0x11e6_0398);
        assert_eq!(adler32(b""), 1);
    }

    #[test]
    fn a_sixteen_colour_bitmap_becomes_a_png() {
        // The depth PuTTY's and 7-Zip's icons use.
        let palette = [[0, 0, 0], [255, 0, 0], [0, 255, 0], [0, 0, 255]];
        let image = bitmap(4, 3, &palette, &[vec![1, 2, 3], vec![3, 0, 1]], &[(1, 1)]);
        let icon = decode(&image).unwrap();
        assert_eq!((icon.width, icon.height, icon.source_bits), (3, 2, 4));
        assert!(!icon.was_png);
        let (width, height, rgba) = read_png(&icon.png);
        assert_eq!((width, height), (3, 2));
        let expected: Vec<u8> = [RED, GREEN, BLUE, BLUE, CLEAR_BLACK, RED].concat();
        assert_eq!(rgba, expected);
    }

    #[test]
    fn every_depth_is_decoded() {
        let palette = [[0, 0, 0], [255, 0, 0]];
        // One bit per pixel: index 1 is red.
        let icon = decode(&bitmap(1, 2, &palette, &[vec![1, 0]], &[(1, 0)])).unwrap();
        assert_eq!(read_png(&icon.png).2, [RED, CLEAR_BLACK].concat());
        // Eight bits per pixel, with a full 256-entry palette.
        let mut full = vec![[0u8, 0, 0]; 256];
        full[200] = [0, 255, 0];
        let icon = decode(&bitmap(8, 1, &full, &[vec![200]], &[])).unwrap();
        assert_eq!(read_png(&icon.png).2, GREEN);
        // Twenty-four bits per pixel: blue, green, red in the file.
        let icon = decode(&bitmap(24, 1, &[], &[vec![0x00ff_0000]], &[])).unwrap();
        assert_eq!(read_png(&icon.png).2, RED);
        // Thirty-two bits with an alpha channel: the mask is not consulted.
        let icon = decode(&bitmap(32, 1, &[], &[vec![0x80ff_0000]], &[(0, 0)])).unwrap();
        assert_eq!(read_png(&icon.png).2, [255, 0, 0, 0x80]);
        // Thirty-two bits with an empty alpha channel: the mask decides.
        let icon = decode(&bitmap(
            32,
            2,
            &[],
            &[vec![0x00ff_0000, 0x0000_00ff]],
            &[(1, 0)],
        ))
        .unwrap();
        assert_eq!(read_png(&icon.png).2, [RED, [0, 0, 255, 0]].concat());
    }

    #[test]
    fn a_large_bitmap_spans_several_stored_blocks() {
        // 256 by 256 pixels is 262 400 bytes of rows: five stored blocks.
        let rows: Vec<Vec<u32>> = (0..256u32)
            .map(|y| (0..256u32).map(|x| 0xff00_0000 | (y << 8) | x).collect())
            .collect();
        let icon = decode(&bitmap(32, 256, &[], &rows, &[])).unwrap();
        let (width, height, rgba) = read_png(&icon.png);
        assert_eq!((width, height), (256, 256));
        // Pixel (x, y) is blue x, green y.
        assert_eq!(&rgba[..4], [0, 0, 0, 255]);
        let at = (255 * 256 + 3) * 4;
        assert_eq!(&rgba[at..at + 4], [0, 255, 3, 255]);
    }

    #[test]
    fn a_png_image_is_passed_through() {
        let inner = encode_png(&GREEN, 1, 1);
        let icon = decode(&inner).unwrap();
        assert!(icon.was_png);
        assert_eq!((icon.width, icon.height), (1, 1));
        assert_eq!(icon.png, inner);
        // A PNG that claims an absurd size is not taken.
        let mut huge = inner.clone();
        huge[16..20].copy_from_slice(&5000u32.to_be_bytes());
        assert!(decode(&huge).is_none());
    }

    #[test]
    fn damaged_bitmaps_are_refused_without_a_panic() {
        let palette = [[0, 0, 0], [255, 255, 255]];
        let whole = bitmap(4, 16, &palette, &vec![vec![1; 16]; 16], &[]);
        assert!(decode(&whole).is_some());
        // Cut anywhere before the mask: no icon. Never a panic.
        let mask_len = 16 * 4;
        for length in 0..whole.len() - mask_len {
            assert!(decode(&whole[..length]).is_none(), "{length} bytes");
        }
        // Without its mask the image is still decoded, fully opaque.
        let icon = decode(&whole[..whole.len() - mask_len]).unwrap();
        assert!(read_png(&icon.png).2.chunks(4).all(|pixel| pixel[3] == 255));
        // Compressed bitmaps, absurd sizes and unknown depths are refused.
        for (at, value) in [(16usize, 1u32), (4, 5000), (8, 5000), (0, 12)] {
            let mut bad = whole.clone();
            bad[at..at + 4].copy_from_slice(&value.to_le_bytes());
            assert!(decode(&bad).is_none(), "field at {at}");
        }
        let mut bad = whole.clone();
        bad[14..16].copy_from_slice(&16u16.to_le_bytes());
        assert!(decode(&bad).is_none());
        assert!(decode(b"").is_none());
        assert!(decode(&PNG_MAGIC).is_none());
    }

    #[test]
    fn the_largest_deepest_image_of_the_first_group_is_extracted() {
        let palette = [[0, 0, 0], [255, 0, 0]];
        let small = bitmap(4, 1, &palette, &[vec![1]], &[]);
        let large_shallow = bitmap(4, 2, &palette, &[vec![1, 1], vec![1, 1]], &[]);
        let large_deep = bitmap(
            32,
            2,
            &[],
            &[
                vec![0xff00_ff00, 0xff00_ff00],
                vec![0xff00_ff00, 0xff00_ff00],
            ],
            &[],
        );
        let resources = vec![
            (RT_ICON, 1, small),
            (RT_ICON, 2, large_shallow),
            (RT_ICON, 3, large_deep),
            (RT_ICON, 4, bitmap(4, 1, &palette, &[vec![0]], &[])),
            (
                RT_GROUP_ICON,
                10,
                group(&[(1, 4, 1), (2, 4, 2), (2, 32, 3)]),
            ),
            (RT_GROUP_ICON, 20, group(&[(1, 4, 4)])),
        ];
        let temp = TempDir::new("icon");
        let path = temp.path().join("program.exe");
        std::fs::write(&path, image(0x8664, &resources, b"")).unwrap();
        let icon = extract_icon(&path).unwrap().unwrap();
        assert_eq!((icon.width, icon.height, icon.source_bits), (2, 2, 32));
        assert_eq!(read_png(&icon.png).2, [GREEN, GREEN, GREEN, GREEN].concat());

        // A program without an icon, and a file that is not a program.
        std::fs::write(&path, image(0x8664, &[], b"")).unwrap();
        assert_eq!(extract_icon(&path).unwrap(), None);
        std::fs::write(&path, b"text").unwrap();
        assert_eq!(extract_icon(&path).unwrap(), None);
        assert!(extract_icon(&temp.path().join("absent")).is_err());
    }
}
