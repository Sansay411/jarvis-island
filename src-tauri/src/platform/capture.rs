//! On-demand screen capture.
//!
//! JARVIS captures single frames only when a task needs them, so GDI is the
//! simplest reliable mechanism: `BitBlt` from the composited desktop for
//! monitors/regions, `PrintWindow(PW_RENDERFULLCONTENT)` for specific windows
//! (works when they are partly covered). Windows Graphics Capture would only
//! pay off for continuous streaming, which JARVIS deliberately never does.
//! The island excludes itself from captures with `WDA_EXCLUDEFROMCAPTURE`
//! for the duration of a capture.

use std::io::Cursor;
use std::time::Instant;

use base64::Engine;
use serde::Serialize;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC,
    SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS, HDC, SRCCOPY,
};
use windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowRect, IsIconic, SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE, WDA_NONE,
};

use super::display::Rect;
use crate::error::{AppError, AppResult};

/// Longest side of images sent to the model.
pub const MAX_SIDE: u32 = 1600;
const PW_RENDERFULLCONTENT: u32 = 2;

pub struct Frame {
    pub width: u32,
    pub height: u32,
    /// RGBA8
    pub pixels: Vec<u8>,
    /// Screen rect (physical px) the frame covers.
    pub rect: Rect,
    pub capture_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EncodedCapture {
    pub data_url: String,
    pub image_width: u32,
    pub image_height: u32,
    /// Screen rect the image covers, physical pixels.
    pub screen_rect: Rect,
    /// Screen pixels per image pixel.
    pub scale: f64,
    pub capture_ms: u64,
    pub encode_ms: u64,
    pub bytes: usize,
}

struct MemDc {
    screen: HDC,
    mem: HDC,
    bitmap: windows::Win32::Graphics::Gdi::HBITMAP,
    old: windows::Win32::Graphics::Gdi::HGDIOBJ,
    w: i32,
    h: i32,
}

impl MemDc {
    fn new(w: i32, h: i32) -> AppResult<MemDc> {
        unsafe {
            let screen = GetDC(None);
            if screen.is_invalid() {
                return Err(AppError::platform("Не удалось получить контекст экрана"));
            }
            let mem = CreateCompatibleDC(Some(screen));
            let bitmap = CreateCompatibleBitmap(screen, w, h);
            let old = SelectObject(mem, bitmap.into());
            Ok(MemDc { screen, mem, bitmap, old, w, h })
        }
    }

    fn read(&self) -> Vec<u8> {
        unsafe {
            let mut bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: self.w,
                    biHeight: -self.h, // top-down rows
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut buf = vec![0u8; (self.w * self.h * 4) as usize];
            GetDIBits(self.mem, self.bitmap, 0, self.h as u32, Some(buf.as_mut_ptr() as *mut _), &mut bmi, DIB_RGB_COLORS);
            // BGRA → RGBA, opaque.
            for px in buf.chunks_exact_mut(4) {
                px.swap(0, 2);
                px[3] = 255;
            }
            buf
        }
    }
}

impl Drop for MemDc {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.mem, self.old);
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.mem);
            ReleaseDC(None, self.screen);
        }
    }
}

/// Hide the island from captures while `f` runs.
fn without_island<T>(island_hwnd: Option<isize>, f: impl FnOnce() -> T) -> T {
    if let Some(h) = island_hwnd {
        unsafe {
            let _ = SetWindowDisplayAffinity(HWND(h as *mut _), WDA_EXCLUDEFROMCAPTURE);
        }
    }
    let out = f();
    if let Some(h) = island_hwnd {
        unsafe {
            let _ = SetWindowDisplayAffinity(HWND(h as *mut _), WDA_NONE);
        }
    }
    out
}

pub fn capture_rect(rect: Rect, island_hwnd: Option<isize>) -> AppResult<Frame> {
    if rect.width <= 0 || rect.height <= 0 {
        return Err(AppError::invalid("Пустая область захвата"));
    }
    let started = Instant::now();
    without_island(island_hwnd, || {
        let dc = MemDc::new(rect.width, rect.height)?;
        unsafe {
            BitBlt(dc.mem, 0, 0, rect.width, rect.height, Some(dc.screen), rect.x, rect.y, SRCCOPY | CAPTUREBLT)?;
        }
        let pixels = dc.read();
        Ok(Frame {
            width: rect.width as u32,
            height: rect.height as u32,
            pixels,
            rect,
            capture_ms: started.elapsed().as_millis() as u64,
        })
    })
}

/// Visible frame of a window (without the invisible resize borders).
pub fn window_frame(hwnd: HWND) -> AppResult<Rect> {
    unsafe {
        let mut r = RECT::default();
        if DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS, &mut r as *mut RECT as *mut _, std::mem::size_of::<RECT>() as u32)
            .is_err()
        {
            GetWindowRect(hwnd, &mut r)?;
        }
        Ok(r.into())
    }
}

/// Capture a window's own contents even if it is partly covered.
pub fn capture_window(hwnd: HWND) -> AppResult<Frame> {
    unsafe {
        if IsIconic(hwnd).as_bool() {
            return Err(AppError::invalid("Окно свёрнуто — сначала его нужно развернуть"));
        }
    }
    let started = Instant::now();
    let mut outer = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut outer)? };
    let frame = window_frame(hwnd)?;
    let (ow, oh) = (outer.right - outer.left, outer.bottom - outer.top);
    let dc = MemDc::new(ow, oh)?;
    let ok = unsafe { PrintWindow(hwnd, dc.mem, PRINT_WINDOW_FLAGS(PW_RENDERFULLCONTENT)) }.as_bool();
    if !ok {
        return Err(AppError::platform("Окно не поддерживает захват содержимого"));
    }
    let full = dc.read();
    // Crop to the visible frame.
    let (cx, cy) = (frame.x - outer.left, frame.y - outer.top);
    let (cw, ch) = (frame.width.min(ow - cx), frame.height.min(oh - cy));
    let mut pixels = Vec::with_capacity((cw * ch * 4) as usize);
    for row in cy..cy + ch {
        let start = ((row * ow + cx) * 4) as usize;
        pixels.extend_from_slice(&full[start..start + (cw * 4) as usize]);
    }
    // PrintWindow returns black for some GPU surfaces; detect and report.
    let sample_dark = pixels.chunks_exact(4).step_by(97).all(|p| p[0] < 4 && p[1] < 4 && p[2] < 4);
    if sample_dark {
        return Err(AppError::platform("Окно отдало пустой кадр (аппаратная отрисовка)"));
    }
    Ok(Frame {
        width: cw as u32,
        height: ch as u32,
        pixels,
        rect: Rect { x: frame.x, y: frame.y, width: cw, height: ch },
        capture_ms: started.elapsed().as_millis() as u64,
    })
}

pub fn encode(frame: Frame, max_side: u32) -> AppResult<EncodedCapture> {
    let started = Instant::now();
    let img = image::RgbaImage::from_raw(frame.width, frame.height, frame.pixels)
        .ok_or_else(|| AppError::internal("Некорректный кадр"))?;
    let img = image::DynamicImage::ImageRgba8(img);
    let img = if frame.width.max(frame.height) > max_side { img.resize(max_side, max_side, image::imageops::FilterType::Triangle) } else { img };
    let rgb = img.to_rgb8();
    let mut out = Cursor::new(Vec::new());
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 82)
        .encode_image(&rgb)
        .map_err(|e| AppError::internal(format!("JPEG: {e}")))?;
    let bytes = out.into_inner();
    Ok(EncodedCapture {
        image_width: rgb.width(),
        image_height: rgb.height(),
        scale: frame.width as f64 / rgb.width() as f64,
        screen_rect: frame.rect,
        capture_ms: frame.capture_ms,
        encode_ms: started.elapsed().as_millis() as u64,
        bytes: bytes.len(),
        data_url: format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(&bytes)),
    })
}
