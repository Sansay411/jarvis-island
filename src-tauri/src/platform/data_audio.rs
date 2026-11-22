use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioState {
    /// 0..=100, rounded from the scalar endpoint level.
    pub volume: u8,
    pub muted: bool,
    pub device: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppAudioSession {
    pub pid: u32,
    pub process: String,
    pub volume: u8,
    pub muted: bool,
}
