use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayBrightness {
    pub id: String,
    pub name: String,
    /// "internal" (WMI) or "external" (DDC/CI)
    pub kind: &'static str,
    pub brightness: Option<u8>,
    pub supported: bool,
}
