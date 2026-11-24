//! User settings, persisted as one JSON document in SQLite. Unknown or
//! missing fields fall back to defaults so older settings keep loading.

use serde::{Deserialize, Serialize};

/// Known OpenAI-compatible endpoints. Jarvis Cloud (DeepSeek Flash with
/// the project's key, no setup) is the default agent.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPreset {
    pub id: &'static str,
    pub label: &'static str,
    pub base_url: &'static str,
    pub default_model: &'static str,
    pub needs_key: bool,
    /// Sends `enable_thinking: false` (DashScope hybrid-thinking models).
    pub dashscope: bool,
}

pub const PRESETS: &[ProviderPreset] = &[
    ProviderPreset {
        id: "jarvis-cloud",
        label: "Jarvis Cloud — бесплатно, без ключа",
        base_url: crate::cloud::API,
        default_model: "deepseek-flash",
        needs_key: false,
        dashscope: false,
    },
    ProviderPreset {
        id: "deepseek",
        label: "DeepSeek V4.1 Flash",
        base_url: "https://api.deepseek.com/v1",
        default_model: "deepseek-flash",
        needs_key: true,
        dashscope: false,
    },
    ProviderPreset {
        id: "dashscope-intl",
        label: "Qwen — Model Studio, Сингапур",
        base_url: "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
        default_model: "qwen3.8-flash",
        needs_key: true,
        dashscope: true,
    },
    ProviderPreset {
        id: "dashscope-us",
        label: "Qwen — Model Studio, США",
        base_url: "https://dashscope-us.aliyuncs.com/compatible-mode/v1",
        default_model: "qwen3.8-flash",
        needs_key: true,
        dashscope: true,
    },
    ProviderPreset {
        id: "dashscope-cn",
        label: "Qwen — Model Studio, Пекин",
        base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1",
        default_model: "qwen3.8-flash",
        needs_key: true,
        dashscope: true,
    },
    ProviderPreset {
        id: "openai",
        label: "OpenAI",
        base_url: "https://api.openai.com/v1",
        default_model: "",
        needs_key: true,
        dashscope: false,
    },
    ProviderPreset {
        id: "openrouter",
        label: "OpenRouter",
        base_url: "https://openrouter.ai/api/v1",
        default_model: "",
        needs_key: true,
        dashscope: false,
    },
    ProviderPreset {
        id: "ollama",
        label: "Ollama (локально)",
        base_url: "http://localhost:11434/v1",
        default_model: "",
        needs_key: false,
        dashscope: false,
    },
    ProviderPreset {
        id: "custom",
        label: "Другой OpenAI-совместимый",
        base_url: "",
        default_model: "",
        needs_key: true,
        dashscope: false,
    },
];

pub fn preset(id: &str) -> &'static ProviderPreset {
    PRESETS.iter().find(|p| p.id == id).unwrap_or(&PRESETS[0])
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderSettings {
    pub preset: String,
    /// Overrides the preset URL (workspace domains, custom gateways).
    pub base_url: String,
    pub model: String,
    /// Model used when a request carries images; empty = same as `model`.
    pub vision_model: String,
    pub temperature: f32,
}

impl Default for ProviderSettings {
    fn default() -> Self {
        let p = &PRESETS[0];
        ProviderSettings {
            preset: p.id.into(),
            base_url: String::new(),
            model: p.default_model.into(),
            vision_model: String::new(),
            temperature: 0.3,
        }
    }
}

impl ProviderSettings {
    pub fn effective_base_url(&self) -> String {
        let url = if self.base_url.trim().is_empty() { preset(&self.preset).base_url } else { self.base_url.trim() };
        url.trim_end_matches('/').to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VoiceSettings {
    pub enabled: bool,
    /// "local": GigaAM on this PC + the chat model + neural speech.
    /// "realtime": Qwen Omni Realtime (DashScope key).
    pub engine: String,
    /// Recognition model id from `voice::models::ASSETS`.
    pub stt_model: String,
    /// Voice id from `voice::tts::voices()`; empty = best downloaded voice.
    pub tts_voice: String,
    /// Speaking rate, 0.5–2.0.
    pub tts_rate: f32,
    /// Pitch adjustment in semitones / percent (-20..=20), default 0.
    pub tts_pitch: i32,
    /// Automatically read assistant chat replies aloud via TTS.
    pub auto_speak_replies: bool,
    /// Keep the microphone open and start a conversation on «Джарвис…».
    pub wake_word: bool,
    /// Realtime WebSocket endpoint (without the `?model=` query).
    pub realtime_url: String,
    pub model: String,
    pub voice: String,
    /// Use the LLM key for voice (same DashScope account).
    pub reuse_llm_key: bool,
    pub input_device: String,
    pub output_device: String,
    /// Make playing apps quieter while JARVIS listens and talks.
    pub duck_media: bool,
    /// Recognise finished requests with Yandex SpeechKit through Jarvis
    /// Cloud as well; the local model stays for the name and as a fallback.
    pub cloud_stt: bool,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        VoiceSettings {
            enabled: true,
            engine: "local".into(),
            stt_model: "gigaam-v3".into(),
            tts_voice: String::new(),
            tts_rate: 1.1,
            tts_pitch: 0,
            auto_speak_replies: false,
            wake_word: false,
            realtime_url: "wss://dashscope-intl.aliyuncs.com/api-ws/v1/realtime".into(),
            model: "qwen3.8-omni-flash-realtime".into(),
            voice: "Katerina".into(),
            reuse_llm_key: true,
            input_device: String::new(),
            output_device: String::new(),
            duck_media: true,
            cloud_stt: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ShortcutSettings {
    pub workspace: String,
    pub voice: String,
    /// The clipboard history. Win+V belongs to Windows.
    pub clipboard: String,
}

impl Default for ShortcutSettings {
    fn default() -> Self {
        // Ctrl+Alt+Space is commonly taken (IME switchers, launchers).
        ShortcutSettings { workspace: "Ctrl+Alt+J".into(), voice: "Ctrl+Alt+K".into(), clipboard: "Ctrl+Alt+V".into() }
    }
}

/// Clipboard history (Settings → Приватность). Everything stays on this PC.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ClipboardSettings {
    /// Record what is copied. Off: the clipboard is not even read.
    pub enabled: bool,
    /// Unpinned entries kept.
    pub max_items: u32,
    /// Days an unpinned entry is kept; 0 keeps it until the limit pushes it out.
    pub keep_days: u32,
    /// Skip text that looks like a password, token or card number.
    pub skip_secrets: bool,
    /// Parts of program names whose copies are never recorded.
    pub ignored_apps: Vec<String>,
}

impl Default for ClipboardSettings {
    fn default() -> Self {
        ClipboardSettings {
            enabled: true,
            max_items: 100,
            keep_days: 7,
            skip_secrets: true,
            ignored_apps: crate::clips::privacy::DEFAULT_IGNORED_APPS.iter().map(|s| s.to_string()).collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PrivacySettings {
    pub store_history: bool,
    /// Allow the agent to take screenshots when a task needs them.
    pub allow_screen: bool,
    /// Allow the agent to read the clipboard without asking each time.
    pub allow_clipboard_read: bool,
}

impl Default for PrivacySettings {
    fn default() -> Self {
        PrivacySettings { store_history: true, allow_screen: true, allow_clipboard_read: false }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AgentSettings {
    pub max_steps: u32,
    /// Ask before moderate-risk actions (file edits, coding agents, closing windows).
    pub confirm_moderate: bool,
    /// «Полное управление»: act without asking, except for what cannot be
    /// taken back or leaves the PC (see `Agent::still_confirmed`).
    pub full_control: bool,
    /// Folders the user works in; used to resolve "my project".
    pub project_roots: Vec<String>,
}

impl Default for AgentSettings {
    fn default() -> Self {
        AgentSettings { max_steps: 24, confirm_moderate: true, full_control: false, project_roots: Vec::new() }
    }
}

/// How JARVIS talks to the user (Settings → Характер).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PersonaSettings {
    /// What JARVIS calls the user; empty = no name.
    pub user_name: String,
    /// "ty" or "vy".
    pub address: String,
    /// "witty", "warm" or "concise".
    pub style: String,
}

impl Default for PersonaSettings {
    fn default() -> Self {
        PersonaSettings { user_name: String::new(), address: "ty".into(), style: "witty".into() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UpdateSettings {
    /// Look for a new version on GitHub soon after start and every six hours.
    pub auto_check: bool,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        UpdateSettings { auto_check: true }
    }
}

/// The planner window and Google Calendar (Settings → Календарь).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PlannerSettings {
    /// Working hours, "HH:MM": «Спланировать день» puts deep work inside them.
    pub work_start: String,
    pub work_end: String,
    /// The hour the day and week views scroll to.
    pub day_start_hour: u32,
    /// OAuth client of the user's own Google Cloud project («Desktop app»).
    pub google_client_id: String,
    /// "primary" or a calendar id.
    pub google_calendar_id: String,
    /// Minutes before an event the island shows it (with «Начать фокус»); 0 = off.
    pub island_remind: u32,
}

impl Default for PlannerSettings {
    fn default() -> Self {
        PlannerSettings {
            work_start: "09:00".into(),
            work_end: "18:00".into(),
            day_start_hour: 8,
            google_client_id: String::new(),
            google_calendar_id: "primary".into(),
            island_remind: 10,
        }
    }
}

/// Words that make a YouTube video «learning» rather than distraction.
pub const LEARNING_WORDS: &[&str] = &[
    "урок", "курс", "лекци", "обучени", "разбор", "туториал", "для начинающих", "как сделать",
    "tutorial", "course", "lecture", "lesson", "learn", "explained", "how to", "crash course",
    "программирован", "programming", "coding", "rust", "javascript", "typescript", "python", "react",
    "алгоритм", "algorithm", "leetcode", "system design", "собеседован", "interview",
    "english", "английск", "grammar", "грамматик", "vocabulary", "ielts", "toefl",
    "тренировк", "workout", "упражнени", "растяжк", "stretching",
];

/// The mentor (Settings → Наставник): short videos are limited, growth is
/// rewarded.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MentorSettings {
    /// Watch short videos and close them once the day's minutes are spent.
    pub guard: bool,
    /// Minutes of TikTok, Shorts, Reels and VK clips a day.
    pub short_minutes: u32,
    /// Minutes of ordinary YouTube a day (learning videos are not counted).
    pub youtube_minutes: u32,
    /// Growth earns extra minutes: a feed set, a workout, a focus session.
    pub earn: bool,
    /// During focus and work, study or project blocks short videos close at once.
    pub strict_focus: bool,
    /// Say it aloud when stepping in.
    pub speak: bool,
    /// What the user grows in: "code", "english", "mind", "body", "projects".
    pub tracks: Vec<String>,
    /// A YouTube title with one of these is learning.
    pub learning_words: Vec<String>,
    /// Quiet hours, "HH:MM" (short videos close at once); empty = off.
    pub night_from: String,
    pub night_to: String,
    /// «Если потянет в короткие видео, то я …»: the user's own plan, shown back.
    pub if_then: String,
}

impl Default for MentorSettings {
    fn default() -> Self {
        MentorSettings {
            guard: true,
            short_minutes: 10,
            youtube_minutes: 30,
            earn: true,
            strict_focus: true,
            speak: true,
            tracks: ["code", "english", "mind", "body", "projects"].iter().map(|s| s.to_string()).collect(),
            learning_words: LEARNING_WORDS.iter().map(|s| s.to_string()).collect(),
            night_from: String::new(),
            night_to: String::new(),
            if_then: String::new(),
        }
    }
}

/// The Telegram bot (Settings → Telegram). The token is in the key store.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TelegramSettings {
    pub enabled: bool,
    /// The one chat the bot serves (bound with «/start CODE»); 0 = none yet.
    pub chat_id: i64,
    pub bot_username: String,
    /// Morning plan and evening review, "HH:MM"; empty = off.
    pub morning: String,
    pub evening: String,
    /// Minutes before an event to remind about it; 0 = off.
    pub remind_before: u32,
    /// Plain messages go to JARVIS as requests.
    pub agent: bool,
}

impl Default for TelegramSettings {
    fn default() -> Self {
        TelegramSettings {
            enabled: true,
            chat_id: 0,
            bot_username: String::new(),
            morning: "08:30".into(),
            evening: "21:30".into(),
            remind_before: 10,
            agent: true,
        }
    }
}

/// First-run «Знакомство» flow state.
pub const ONBOARDING_VERSION: u32 = 1;
/// Stop auto-opening after this many «позже» closes.
pub const ONBOARDING_MAX_DISMISSALS: u32 = 3;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct OnboardingSettings {
    /// Completed (or permanently hidden).
    pub done: bool,
    /// Flow version completed; 0 = not completed yet.
    pub version: u32,
    /// How many times the user closed the window as «позже».
    pub dismissed_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub provider: ProviderSettings,
    pub voice: VoiceSettings,
    pub persona: PersonaSettings,
    pub shortcuts: ShortcutSettings,
    pub privacy: PrivacySettings,
    pub clipboard: ClipboardSettings,
    pub agent: AgentSettings,
    pub launch_at_startup: bool,
    pub reduce_motion: bool,
    pub updates: UpdateSettings,
    pub planner: PlannerSettings,
    pub mentor: MentorSettings,
    pub telegram: TelegramSettings,
    pub onboarding: OnboardingSettings,
}

/// Whether the first-run «Знакомство» window should open on island start.
/// Existing users who already set a persona name or have saved memories are
/// treated as done so they are never interrupted.
pub fn should_auto_open_onboarding(settings: &Settings, memories_count: usize) -> bool {
    if settings.onboarding.done {
        return false;
    }
    if settings.onboarding.dismissed_count >= ONBOARDING_MAX_DISMISSALS {
        return false;
    }
    if !settings.persona.user_name.trim().is_empty() {
        return false;
    }
    if memories_count > 0 {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_settings_json_defaults_onboarding() {
        let s: Settings = serde_json::from_str(r#"{"persona":{"userName":""}}"#).unwrap();
        assert!(!s.onboarding.done);
        assert_eq!(s.onboarding.version, 0);
        assert_eq!(s.onboarding.dismissed_count, 0);
        assert!(should_auto_open_onboarding(&s, 0));
    }

    #[test]
    fn onboarding_trigger_skips_configured_and_dismissed_users() {
        let mut s = Settings::default();
        assert!(should_auto_open_onboarding(&s, 0));

        // Existing memories mean an established user.
        assert!(!should_auto_open_onboarding(&s, 1));

        // Non-empty persona name means an established user; whitespace alone does not.
        s.persona.user_name = "   ".into();
        assert!(should_auto_open_onboarding(&s, 0));
        s.persona.user_name = "Алексей".into();
        assert!(!should_auto_open_onboarding(&s, 0));
        s.persona.user_name.clear();

        // Closing as «позже» asks again up to 3 times.
        s.onboarding.dismissed_count = 2;
        assert!(should_auto_open_onboarding(&s, 0));
        s.onboarding.dismissed_count = 3;
        assert!(!should_auto_open_onboarding(&s, 0));

        // Completed never auto-opens again.
        s.onboarding.dismissed_count = 0;
        s.onboarding.done = true;
        assert!(!should_auto_open_onboarding(&s, 0));
    }
}
