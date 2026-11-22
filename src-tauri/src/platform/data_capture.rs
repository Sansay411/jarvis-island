use super::display::Rect;
use serde::Serialize;

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
