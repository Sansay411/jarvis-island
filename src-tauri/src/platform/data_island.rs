use serde::Deserialize;
use serde::Serialize;

/// Island shape in logical pixels relative to the canvas window's client area.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
pub struct ShapeRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PointerEvent {
    pub inside: bool,
    pub drag_approach: bool,
}

#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct PresenceEvent {
    pub visible: bool,
    pub reason: &'static str,
}
