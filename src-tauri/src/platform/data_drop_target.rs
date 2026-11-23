use serde::Serialize;

#[derive(Serialize, Clone)]
pub struct FileDropEvent {
    /// enter | over | leave | drop
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub paths: Vec<String>,
    /// Cursor inside the island in physical pixels (enter/over/drop).
    pub x: Option<i32>,
    pub y: Option<i32>,
}
