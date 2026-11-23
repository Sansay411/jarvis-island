#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardContent {
    pub text: Option<String>,
    pub files: Vec<String>,
}

/// What the history sees on the clipboard right now.
#[derive(Debug, Default)]
pub struct Snapshot {
    pub sequence: u32,
    pub owner_pid: Option<u32>,
    /// The program that copied asked for this to stay out of histories
    /// (password managers do). Nothing else was read.
    pub private: bool,
    pub text: Option<String>,
    pub files: Vec<String>,
    /// Raw clipboard formats of a picture: "PNG" and CF_DIBV5 / CF_DIB.
    pub png: Option<Vec<u8>>,
    pub dib: Option<Vec<u8>>,
}
