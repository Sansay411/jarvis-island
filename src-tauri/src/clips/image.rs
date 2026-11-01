//! Pictures on the clipboard: Windows hands them over as DIBs (or as raw PNG
//! for some programs). They are kept as PNG, identified by their pixels and
//! put back as PNG plus a plain 24-bit DIB that every program understands.

use sha2::{Digest, Sha256};

/// More than this many pixels is not a clipboard picture worth keeping.
pub const MAX_PIXELS: u64 = 40_000_000;

/// Decoded picture: `pixels` are RGBA, row by row from the top.
#[derive(Debug, Clone, PartialEq)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// A clipboard DIB (BITMAPINFOHEADER, V4 or V5 header, then pixels).
/// Supports 24-bit, and 32-bit with or without bit masks; anything else is
/// `None`.
pub fn dib_to_rgba(dib: &[u8]) -> Option<Rgba> {
    let size = le_u32(dib, 0)?;
    if !matches!(size, 40 | 108 | 124) {
        return None;
    }
    let width = le_u32(dib, 4)? as i32;
    let raw_height = le_u32(dib, 8)? as i32;
    let bpp = le_u16(dib, 14)?;
    let compression = le_u32(dib, 16)?;
    let colors = le_u32(dib, 32)? as usize;
    if width <= 0 || raw_height == 0 {
        return None;
    }
    let (w, h) = (width as u64, u64::from(raw_height.unsigned_abs()));
    if w * h > MAX_PIXELS {
        return None;
    }
    let (w, h) = (w as usize, h as usize);
    let top_down = raw_height < 0;
    // Channel masks (R, G, B, A) and where the pixels start.
    let table = size as usize + colors.checked_mul(4)?;
    let (masks, offset) = match (bpp, compression) {
        (24, 0) => ([0; 4], table),
        (32, 0) => ([0x00FF_0000, 0x0000_FF00, 0x0000_00FF, 0], table),
        (32, 3) if size == 40 => ([le_u32(dib, 40)?, le_u32(dib, 44)?, le_u32(dib, 48)?, 0], 52),
        (32, 3) => ([le_u32(dib, 40)?, le_u32(dib, 44)?, le_u32(dib, 48)?, le_u32(dib, 52)?], size as usize),
        _ => return None,
    };
    let row = (w * usize::from(bpp)).div_ceil(32) * 4;
    if dib.len() < offset.checked_add(row.checked_mul(h)?)? {
        return None;
    }
    let mut pixels = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        let src = if top_down { y } else { h - 1 - y };
        let line = &dib[offset + src * row..][..row];
        for x in 0..w {
            if bpp == 24 {
                let p = &line[x * 3..x * 3 + 3];
                pixels.extend([p[2], p[1], p[0], 255]);
            } else {
                let v = u32::from_le_bytes(line[x * 4..x * 4 + 4].try_into().ok()?);
                let alpha = if masks[3] == 0 { 255 } else { channel(v, masks[3]) };
                pixels.extend([channel(v, masks[0]), channel(v, masks[1]), channel(v, masks[2]), alpha]);
            }
        }
    }
    // A mask for alpha that nobody filled in means "no transparency".
    if masks[3] != 0 && pixels.chunks_exact(4).all(|p| p[3] == 0) {
        pixels.chunks_exact_mut(4).for_each(|p| p[3] = 255);
    }
    Some(Rgba { width: w as u32, height: h as u32, pixels })
}

fn le_u32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn le_u16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

/// One channel of a pixel, scaled to 8 bits.
fn channel(value: u32, mask: u32) -> u8 {
    if mask == 0 {
        return 0;
    }
    let shift = mask.trailing_zeros();
    let max = (1u64 << (mask >> shift).count_ones()) - 1;
    (u64::from((value & mask) >> shift) * 255 / max) as u8
}

pub fn png_to_rgba(png: &[u8]) -> Option<Rgba> {
    // The size is in the IHDR chunk; read it before decoding anything big.
    if png.len() < 24 || &png[1..4] != b"PNG" {
        return None;
    }
    let w = u64::from(u32::from_be_bytes(png[16..20].try_into().ok()?));
    let h = u64::from(u32::from_be_bytes(png[20..24].try_into().ok()?));
    if w == 0 || h == 0 || w * h > MAX_PIXELS {
        return None;
    }
    let decoded = image::load_from_memory_with_format(png, image::ImageFormat::Png).ok()?.to_rgba8();
    Some(Rgba { width: decoded.width(), height: decoded.height(), pixels: decoded.into_raw() })
}

pub fn rgba_to_png(img: &Rgba) -> Option<Vec<u8>> {
    let buffer = image::RgbaImage::from_raw(img.width, img.height, img.pixels.clone())?;
    let mut out = Vec::new();
    buffer.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).ok()?;
    Some(out)
}

/// Identifies the pixels, whatever program or format they came from.
pub fn pixel_hash(img: &Rgba) -> String {
    let mut hasher = Sha256::new();
    hasher.update(img.width.to_le_bytes());
    hasher.update(img.height.to_le_bytes());
    hasher.update(&img.pixels);
    hasher.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// PNG data URL that fits `max_w × max_h` (never enlarged).
pub fn thumbnail_data_url(img: &Rgba, max_w: u32, max_h: u32) -> Option<String> {
    use base64::Engine;
    let scale = (f64::from(max_w) / f64::from(img.width)).min(f64::from(max_h) / f64::from(img.height)).min(1.0);
    let small = if scale < 1.0 {
        let (w, h) = (((f64::from(img.width) * scale).round() as u32).max(1), ((f64::from(img.height) * scale).round() as u32).max(1));
        let source = image::RgbaImage::from_raw(img.width, img.height, img.pixels.clone())?;
        let resized = image::imageops::resize(&source, w, h, image::imageops::FilterType::Triangle);
        Rgba { width: w, height: h, pixels: resized.into_raw() }
    } else {
        img.clone()
    };
    let png = rgba_to_png(&small)?;
    Some(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png)))
}

/// 24-bit bottom-up DIB for CF_DIB. Transparency is flattened onto white,
/// as Windows itself does when it converts for older programs.
pub fn rgba_to_dib24(img: &Rgba) -> Vec<u8> {
    let (w, h) = (img.width as usize, img.height as usize);
    let row = (w * 3).div_ceil(4) * 4;
    let mut dib = Vec::with_capacity(40 + row * h);
    dib.extend(40u32.to_le_bytes());
    dib.extend((img.width as i32).to_le_bytes());
    dib.extend((img.height as i32).to_le_bytes());
    dib.extend(1u16.to_le_bytes());
    dib.extend(24u16.to_le_bytes());
    dib.extend(0u32.to_le_bytes());
    dib.extend(((row * h) as u32).to_le_bytes());
    dib.extend([0u8; 16]);
    let flatten = |c: u8, a: u8| ((u32::from(c) * u32::from(a) + 255 * (255 - u32::from(a))) / 255) as u8;
    for y in (0..h).rev() {
        for x in 0..w {
            let p = &img.pixels[(y * w + x) * 4..][..4];
            dib.extend([flatten(p[2], p[3]), flatten(p[1], p[3]), flatten(p[0], p[3])]);
        }
        dib.extend(std::iter::repeat_n(0u8, row - w * 3));
    }
    dib
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(size: u32, w: i32, h: i32, bpp: u16, compression: u32) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend(size.to_le_bytes());
        v.extend(w.to_le_bytes());
        v.extend(h.to_le_bytes());
        v.extend(1u16.to_le_bytes());
        v.extend(bpp.to_le_bytes());
        v.extend(compression.to_le_bytes());
        v.extend([0u8; 20]);
        v
    }

    #[test]
    fn a_bottom_up_24_bit_dib_becomes_rgba_top_down() {
        // 2×2: red, green on top; blue, white below. Rows are padded to 4 bytes.
        let mut dib = header(40, 2, 2, 24, 0);
        dib.extend([255, 0, 0, 255, 255, 255, 0, 0]); // bottom row: blue, white (BGR) + pad
        dib.extend([0, 0, 255, 0, 255, 0, 0, 0]); // top row: red, green + pad
        let img = dib_to_rgba(&dib).unwrap();
        assert_eq!((img.width, img.height), (2, 2));
        assert_eq!(img.pixels, [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255]);
    }

    #[test]
    fn a_top_down_dib_with_negative_height_keeps_its_row_order() {
        let mut dib = header(40, 1, -2, 32, 0);
        dib.extend([0, 0, 255, 0]); // first row: red
        dib.extend([255, 0, 0, 0]); // second row: blue
        let img = dib_to_rgba(&dib).unwrap();
        assert_eq!(img.pixels, [255, 0, 0, 255, 0, 0, 255, 255]);
    }

    #[test]
    fn a_32_bit_dib_without_masks_is_opaque_even_when_the_fourth_byte_is_zero() {
        let mut dib = header(40, 1, 1, 32, 0);
        dib.extend([10, 20, 30, 0]);
        assert_eq!(dib_to_rgba(&dib).unwrap().pixels, [30, 20, 10, 255]);
    }

    fn v4(w: i32, h: i32, masks: [u32; 4]) -> Vec<u8> {
        let mut dib = header(108, w, h, 32, 3);
        for m in masks {
            dib.extend(m.to_le_bytes());
        }
        dib.extend([0u8; 52]);
        dib
    }

    #[test]
    fn a_v4_dib_keeps_the_alpha_it_declares() {
        let mut dib = v4(1, 1, [0x00FF_0000, 0x0000_FF00, 0x0000_00FF, 0xFF00_0000]);
        dib.extend([30, 20, 10, 128]); // B, G, R, A
        assert_eq!(dib_to_rgba(&dib).unwrap().pixels, [10, 20, 30, 128]);
    }

    #[test]
    fn a_picture_whose_alpha_is_zero_everywhere_is_opaque() {
        let mut dib = v4(2, 1, [0x00FF_0000, 0x0000_FF00, 0x0000_00FF, 0xFF00_0000]);
        dib.extend([30, 20, 10, 0, 1, 2, 3, 0]);
        let img = dib_to_rgba(&dib).unwrap();
        assert_eq!(img.pixels, [10, 20, 30, 255, 3, 2, 1, 255]);
    }

    #[test]
    fn bitfields_with_the_plain_header_read_their_masks_after_it() {
        let mut dib = header(40, 1, 1, 32, 3);
        for m in [0x00FF_0000u32, 0x0000_FF00, 0x0000_00FF] {
            dib.extend(m.to_le_bytes());
        }
        dib.extend([30, 20, 10, 0]);
        assert_eq!(dib_to_rgba(&dib).unwrap().pixels, [10, 20, 30, 255]);
    }

    #[test]
    fn broken_or_unsupported_dibs_are_rejected() {
        assert!(dib_to_rgba(&[]).is_none());
        assert!(dib_to_rgba(&header(40, 2, 2, 24, 0)).is_none(), "pixels are missing");
        let mut indexed = header(40, 1, 1, 8, 0);
        indexed.extend([0; 8]);
        assert!(dib_to_rgba(&indexed).is_none(), "palette pictures are not supported");
        let mut huge = header(40, 100_000, 100_000, 24, 0);
        huge.extend([0; 16]);
        assert!(dib_to_rgba(&huge).is_none());
        let mut zero = header(40, 0, 0, 24, 0);
        zero.extend([0; 4]);
        assert!(dib_to_rgba(&zero).is_none());
    }

    fn sample() -> Rgba {
        Rgba { width: 2, height: 2, pixels: vec![255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 0, 9, 8, 7, 255] }
    }

    #[test]
    fn png_keeps_every_pixel_including_alpha() {
        let png = rgba_to_png(&sample()).unwrap();
        assert_eq!(&png[1..4], b"PNG");
        assert_eq!(png_to_rgba(&png).unwrap(), sample());
    }

    #[test]
    fn garbage_is_not_a_png() {
        assert!(png_to_rgba(b"not a picture").is_none());
    }

    #[test]
    fn equal_pixels_hash_alike_and_different_ones_do_not() {
        let a = sample();
        let mut b = sample();
        assert_eq!(pixel_hash(&a), pixel_hash(&b));
        b.pixels[0] = 254;
        assert_ne!(pixel_hash(&a), pixel_hash(&b));
        let wide = Rgba { width: 4, height: 1, pixels: sample().pixels };
        assert_ne!(pixel_hash(&a), pixel_hash(&wide), "the same bytes in another shape are another picture");
    }

    fn solid(w: u32, h: u32) -> Rgba {
        Rgba { width: w, height: h, pixels: [10, 120, 200, 255].repeat((w * h) as usize) }
    }

    fn thumb_size(img: &Rgba, w: u32, h: u32) -> (u32, u32) {
        let url = thumbnail_data_url(img, w, h).unwrap();
        let b64 = url.strip_prefix("data:image/png;base64,").expect("a PNG data URL");
        use base64::Engine;
        let png = base64::engine::general_purpose::STANDARD.decode(b64).unwrap();
        let t = png_to_rgba(&png).unwrap();
        (t.width, t.height)
    }

    #[test]
    fn thumbnails_fit_the_box_keep_the_ratio_and_never_enlarge() {
        assert_eq!(thumb_size(&solid(200, 100), 80, 40), (80, 40));
        assert_eq!(thumb_size(&solid(100, 200), 80, 40), (20, 40));
        assert_eq!(thumb_size(&solid(10, 10), 80, 40), (10, 10));
    }

    #[test]
    fn a_dib_written_back_reads_the_same_and_transparency_turns_white() {
        let opaque = Rgba { width: 3, height: 2, pixels: (0..6u8).flat_map(|i| [i * 40, 255 - i * 40, i, 255]).collect() };
        assert_eq!(dib_to_rgba(&rgba_to_dib24(&opaque)).unwrap(), opaque);
        let clear = Rgba { width: 1, height: 1, pixels: vec![255, 0, 0, 0] };
        assert_eq!(dib_to_rgba(&rgba_to_dib24(&clear)).unwrap().pixels, [255, 255, 255, 255]);
    }
}
