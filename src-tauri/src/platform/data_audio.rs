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

/// An app made quieter while JARVIS listens and talks, and what to put back.
#[derive(Debug, Clone, PartialEq, serde::Deserialize, Serialize)]
pub struct Ducked {
    pub pid: u32,
    pub process: String,
    pub original: f32,
    pub ducked: f32,
}

/// The level a ducked app gets: a fraction of its own, never louder.
pub fn ducked_level(original: f32, factor: f32) -> f32 {
    (original * factor).clamp(0.0, original)
}

/// Whether to put `original` back: only if the level is still the one we set.
/// A user who moved the slider meanwhile keeps their choice.
pub fn still_ducked(current: f32, d: &Ducked) -> bool {
    (current - d.ducked).abs() < 0.02
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ducking_lowers_and_restores_only_what_it_set() {
        assert!((ducked_level(0.8, 0.3) - 0.24).abs() < 1e-6);
        assert_eq!(ducked_level(0.5, 1.5), 0.5);
        let d = Ducked { pid: 1, process: "music.exe".into(), original: 0.8, ducked: 0.24 };
        assert!(still_ducked(0.24, &d));
        assert!(still_ducked(0.25, &d));
        // The user moved the slider while ducked: theirs stays.
        assert!(!still_ducked(0.6, &d));
    }
}
