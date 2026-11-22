pub use super::data_capture::*;
use super::native;
use crate::error::{AppError, AppResult};
use crate::platform::{display::Rect, HWND};
use base64::Engine;
use core_foundation::{
    array::CFArray,
    base::{CFType, TCFType},
    dictionary::CFDictionary,
    number::CFNumber,
    string::CFString,
};
use core_graphics::{
    base::{kCGBitmapByteOrder32Big, kCGImageAlphaPremultipliedLast},
    color_space::CGColorSpace,
    context::CGContext,
    geometry::{CGPoint, CGRect, CGSize},
    image::CGImage,
    window,
};
use std::{io::Cursor, os::unix::fs::DirBuilderExt, path::PathBuf, time::Instant};

static COMMAND_CAPTURE: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

pub const MAX_SIDE: u32 = 1600;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
}

fn require_permission(allowed: bool) -> AppResult<()> {
    if allowed {
        return Ok(());
    }
    Err(AppError::PermissionDenied("Разрешите Джарвису запись экрана: «Системные настройки» → «Конфиденциальность и безопасность» → «Запись экрана» (System Settings › Privacy & Security › Screen Recording), затем перезапустите приложение.".into()))
}

fn validate_rect(rect: Rect) -> AppResult<()> {
    if rect.width <= 0 || rect.height <= 0 {
        return Err(AppError::invalid(
            "Область снимка должна иметь положительную ширину и высоту",
        ));
    }
    Ok(())
}

fn cg_rect(rect: Rect) -> CGRect {
    CGRect::new(
        &CGPoint::new(rect.x as f64, rect.y as f64),
        &CGSize::new(rect.width as f64, rect.height as f64),
    )
}

pub fn capture_rect(rect: Rect, island_hwnd: Option<isize>) -> AppResult<Frame> {
    let excluded = island_hwnd
        .filter(|id| *id != 0)
        .map(|id| window_id(HWND(id)))
        .transpose()?;
    capture(rect, None, excluded)
}

pub fn window_frame(hwnd: HWND) -> AppResult<Rect> {
    let id = window_id(hwnd)?;
    let raw = window::copy_window_info(window::kCGWindowListOptionAll, window::kCGNullWindowID)
        .ok_or_else(|| AppError::platform("Не удалось получить список окон macOS"))?;
    type Dict = CFDictionary<CFString, CFType>;
    let list: CFArray<Dict> = unsafe { TCFType::wrap_under_get_rule(raw.as_concrete_TypeRef()) };
    let description = list
        .iter()
        .find(|d| {
            d.find(CFString::new("kCGWindowNumber"))
                .and_then(|v| v.downcast::<CFNumber>())
                .and_then(|n| n.to_i64())
                == Some(id as i64)
        })
        .ok_or_else(|| AppError::not_found("Окно не найдено"))?;
    let bounds = description
        .find(CFString::new("kCGWindowBounds"))
        .and_then(|v| v.downcast::<CFDictionary>())
        .ok_or_else(|| AppError::platform("Нет границ окна"))?;
    let bounds: Dict = unsafe { TCFType::wrap_under_get_rule(bounds.as_concrete_TypeRef()) };
    let number = |key| -> AppResult<f64> {
        bounds
            .find(CFString::new(key))
            .and_then(|v| v.downcast::<CFNumber>())
            .and_then(|n| n.to_f64())
            .filter(|n| n.is_finite())
            .ok_or_else(|| AppError::platform("Некорректные границы окна"))
    };
    let x = number("X")?;
    let y = number("Y")?;
    let rect = Rect {
        x: x.floor() as i32,
        y: y.floor() as i32,
        width: ((x + number("Width")?).ceil() - x.floor()) as i32,
        height: ((y + number("Height")?).ceil() - y.floor()) as i32,
    };
    validate_rect(rect)?;
    Ok(rect)
}

fn window_id(hwnd: HWND) -> AppResult<u32> {
    u32::try_from(hwnd.0)
        .ok()
        .filter(|id| *id != 0)
        .ok_or_else(|| AppError::invalid("Некорректный идентификатор окна"))
}

pub fn capture_window(hwnd: HWND) -> AppResult<Frame> {
    capture(window_frame(hwnd)?, Some(window_id(hwnd)?), None)
}

fn capture_options(window_id: Option<u32>, excluded: Option<u32>) -> (u32, u32) {
    match (window_id, excluded) {
        (Some(id), _) => (window::kCGWindowListOptionIncludingWindow, id),
        (_, Some(id)) => (window::kCGWindowListOptionOnScreenBelowWindow, id),
        _ => (
            window::kCGWindowListOptionOnScreenOnly,
            window::kCGNullWindowID,
        ),
    }
}

fn capture(rect: Rect, window_id: Option<u32>, excluded: Option<u32>) -> AppResult<Frame> {
    validate_rect(rect)?;
    require_permission(unsafe { CGPreflightScreenCaptureAccess() })?;
    let started = Instant::now();
    let (options, relative_to) = capture_options(window_id, excluded);
    let image = window::create_image(
        cg_rect(rect),
        options,
        relative_to,
        window::kCGWindowImageBoundsIgnoreFraming | window::kCGWindowImageBestResolution,
    );
    let (width, height, pixels) = if let Some(image) = image {
        rgba(&image)?
    } else {
        // Recent macOS versions can disable the deprecated CGWindowListCreateImage API.
        capture_command(rect, window_id, excluded)?
    };
    Ok(Frame {
        width,
        height,
        pixels,
        rect,
        capture_ms: started.elapsed().as_millis() as u64,
    })
}

fn rgba(image: &CGImage) -> AppResult<(u32, u32, Vec<u8>)> {
    let (width, height) = (image.width(), image.height());
    if width == 0 || height == 0 || width.checked_mul(height).is_none_or(|n| n > 100_000_000) {
        return Err(AppError::platform("Некорректный размер снимка"));
    }
    let mut context = CGContext::create_bitmap_context(
        None,
        width,
        height,
        8,
        width * 4,
        &CGColorSpace::create_device_rgb(),
        kCGImageAlphaPremultipliedLast | kCGBitmapByteOrder32Big,
    );
    context.draw_image(
        CGRect::new(
            &CGPoint::new(0.0, 0.0),
            &CGSize::new(width as f64, height as f64),
        ),
        image,
    );
    let mut pixels = context.data().to_vec();
    for pixel in pixels.chunks_exact_mut(4) {
        let alpha = pixel[3] as u32;
        if alpha > 0 && alpha < 255 {
            for channel in &mut pixel[..3] {
                *channel = ((*channel as u32 * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
    }
    Ok((width as u32, height as u32, pixels))
}

struct CaptureDir(PathBuf);
impl Drop for CaptureDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn capture_command(
    rect: Rect,
    window_id: Option<u32>,
    excluded: Option<u32>,
) -> AppResult<(u32, u32, Vec<u8>)> {
    let _capture_lock = COMMAND_CAPTURE.lock();
    let _visibility = excluded
        .map(|id| super::island::hide_for_capture(id as isize))
        .transpose()?;
    let dir =
        CaptureDir(std::env::temp_dir().join(format!("jarvis-capture-{}", uuid::Uuid::new_v4())));
    std::fs::DirBuilder::new().mode(0o700).create(&dir.0)?;
    let path = dir.0.join("capture.png");
    let target = window_id
        .map(|id| format!("-l{id}"))
        .unwrap_or_else(|| format!("-R{},{},{},{}", rect.x, rect.y, rect.width, rect.height));
    native::execute(
        "/usr/sbin/screencapture",
        &[
            "-x",
            "-o",
            "-t",
            "png",
            &target,
            path.to_str()
                .ok_or_else(|| AppError::platform("Некорректный путь снимка"))?,
        ],
        None,
    )
    .map_err(|error| {
        require_permission(unsafe { CGPreflightScreenCaptureAccess() })
            .err()
            .unwrap_or(error)
    })?;
    let image = image::ImageReader::open(&path)?
        .decode()
        .map_err(|e| AppError::platform(format!("PNG: {e}")))?
        .to_rgba8();
    Ok((image.width(), image.height(), image.into_raw()))
}

pub fn encode(frame: Frame, max_side: u32) -> AppResult<EncodedCapture> {
    validate_rect(frame.rect)?;
    if max_side == 0 || frame.width == 0 || frame.height == 0 {
        return Err(AppError::invalid("Размер снимка должен быть больше нуля"));
    }
    let started = Instant::now();
    let image = image::RgbaImage::from_raw(frame.width, frame.height, frame.pixels)
        .ok_or_else(|| AppError::internal("Некорректный кадр"))?;
    let image = image::DynamicImage::ImageRgba8(image);
    let image = if frame.width.max(frame.height) > max_side {
        image.resize(max_side, max_side, image::imageops::FilterType::Triangle)
    } else {
        image
    };
    let mut out = Cursor::new(Vec::new());
    image
        .write_to(&mut out, image::ImageFormat::Png)
        .map_err(|e| AppError::internal(format!("PNG: {e}")))?;
    let bytes = out.into_inner();
    Ok(EncodedCapture {
        image_width: image.width(),
        image_height: image.height(),
        // Convert image pixels back to Quartz points, including Retina captures.
        scale: frame.rect.width as f64 / image.width() as f64,
        screen_rect: frame.rect,
        capture_ms: frame.capture_ms,
        encode_ms: started.elapsed().as_millis() as u64,
        bytes: bytes.len(),
        data_url: format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&bytes)
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capture_options_exclude_only_for_screen_and_region_targets() {
        assert_eq!(
            capture_options(None, None),
            (
                window::kCGWindowListOptionOnScreenOnly,
                window::kCGNullWindowID
            )
        );
        assert_eq!(
            capture_options(None, Some(42)),
            (window::kCGWindowListOptionOnScreenBelowWindow, 42)
        );
        assert_eq!(
            capture_options(Some(7), None),
            (window::kCGWindowListOptionIncludingWindow, 7)
        );
        assert_eq!(
            capture_options(Some(7), Some(42)),
            (window::kCGWindowListOptionIncludingWindow, 7)
        );
    }
    #[test]
    fn fallback_refuses_to_capture_if_the_island_cannot_be_hidden() {
        assert!(capture_command(
            Rect {
                width: 1,
                height: 1,
                ..Rect::default()
            },
            None,
            Some(u32::MAX)
        )
        .is_err());
    }
    #[test]
    #[ignore = "Requires a logged-in macOS desktop and Screen Recording permission"]
    fn macos_command_capture_live() {
        require_permission(unsafe { CGPreflightScreenCaptureAccess() }).unwrap();
        let rect = Rect {
            x: 0,
            y: 0,
            width: 128,
            height: 96,
        };
        let (width, height, pixels) = capture_command(rect, None, None).unwrap();
        let encoded = encode(
            Frame {
                width,
                height,
                pixels,
                rect,
                capture_ms: 0,
            },
            MAX_SIDE,
        )
        .unwrap();
        assert!(encoded.bytes > 0);
        println!(
            "screencapture fallback: PNG {}x{}, {} bytes",
            encoded.image_width, encoded.image_height, encoded.bytes
        );
    }
    fn frame() -> Frame {
        Frame {
            width: 4,
            height: 2,
            pixels: vec![255; 32],
            rect: Rect {
                x: -10,
                y: 20,
                width: 2,
                height: 1,
            },
            capture_ms: 7,
        }
    }
    #[test]
    fn bitmap_conversion_preserves_orientation_color_and_alpha() {
        let mut source = CGContext::create_bitmap_context(
            None,
            2,
            2,
            8,
            8,
            &CGColorSpace::create_device_rgb(),
            kCGImageAlphaPremultipliedLast | kCGBitmapByteOrder32Big,
        );
        source.data().copy_from_slice(&[
            128, 0, 0, 128, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ]);
        let (width, height, pixels) = rgba(&source.create_image().unwrap()).unwrap();
        assert_eq!((width, height), (2, 2));
        assert_eq!(
            pixels,
            [255, 0, 0, 128, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255]
        );
    }
    #[test]
    fn png_encoding_preserves_geometry_and_retina_scale() {
        for (limit, width, scale) in [(10, 4, 0.5), (2, 2, 1.0)] {
            let encoded = encode(frame(), limit).unwrap();
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(
                    encoded
                        .data_url
                        .strip_prefix("data:image/png;base64,")
                        .unwrap(),
                )
                .unwrap();
            assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
            let image = image::load_from_memory(&bytes).unwrap();
            assert_eq!((image.width(), image.height()), (width, width / 2));
            assert_eq!(encoded.bytes, bytes.len());
            assert_eq!(encoded.scale, scale);
            assert_eq!(encoded.screen_rect, frame().rect);
            assert_eq!(encoded.capture_ms, 7);
        }
    }
    #[test]
    fn rejects_invalid_and_empty_input() {
        assert!(encode(frame(), 0).is_err());
        let mut empty = frame();
        empty.pixels.clear();
        assert!(encode(empty, MAX_SIDE).is_err());
        let mut empty = frame();
        empty.width = 0;
        assert!(encode(empty, MAX_SIDE).is_err());
        assert!(capture_rect(Rect::default(), None).is_err());
        assert!(capture_rect(
            Rect {
                width: 1,
                height: 1,
                ..Rect::default()
            },
            Some(-1)
        )
        .is_err());
        assert!(window_frame(HWND(0)).is_err());
        assert!(window_frame(HWND(-1)).is_err());
        assert!(validate_rect(Rect {
            width: -1,
            height: 1,
            ..Rect::default()
        })
        .is_err());
    }
    #[test]
    fn permission_error_explains_how_to_enable_screen_recording() {
        assert!(require_permission(true).is_ok());
        let Err(AppError::PermissionDenied(message)) = require_permission(false) else {
            panic!("expected permission error")
        };
        assert!(message.contains("Запись экрана") && message.contains("Privacy & Security"));
    }
}
