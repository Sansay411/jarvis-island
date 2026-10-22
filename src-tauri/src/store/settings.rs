//! User settings, persisted as one JSON document in SQLite. Unknown or
//! missing fields fall back to defaults so older settings keep loading.

use serde::{Deserialize, Serialize};

/// Known OpenAI-compatible endpoints. Model ids for DashScope come from the
/// Model Studio model list (qwen3.8-flash / qwen3.8-max, Oct 2026); other
/// presets leave the model for the user to choose.
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
    // Alem AI: LiteLLM proxy in front of vLLM serving Qwen3.8 27B (text +
    // images, streamed tool calls, reasoning in `reasoning_content`).
    // LiteLLM rejects unknown parameters, so nothing provider-specific is sent.
    ProviderPreset {
        id: "alem",
        label: "Qwen3.8 — Alem AI",
        base_url: "https://llm.alem.ai/v1",
        default_model: "qwen3-8",
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
    /// "local": Whisper on this PC + the chat model + a Windows voice.
    /// "realtime": Qwen Omni Realtime (DashScope key).
    pub engine: String,
    /// Recognition model id from `voice::models::ASSETS`.
    pub stt_model: String,
    /// Voice id from `voice::tts::voices()`; empty = best downloaded voice.
    pub tts_voice: String,
    /// Speaking rate, 0.5–2.0.
    pub tts_rate: f32,
    /// Realtime WebSocket endpoint (without the `?model=` query).
    pub realtime_url: String,
    pub model: String,
    pub voice: String,
    /// Use the LLM key for voice (same DashScope account).
    pub reuse_llm_key: bool,
    pub input_device: String,
    pub output_device: String,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        VoiceSettings {
            enabled: true,
            engine: "local".into(),
            stt_model: "gigaam-v3".into(),
            tts_voice: String::new(),
            tts_rate: 1.1,
            realtime_url: "wss://dashscope-intl.aliyuncs.com/api-ws/v1/realtime".into(),
            model: "qwen3.8-omni-flash-realtime".into(),
            voice: "Katerina".into(),
            reuse_llm_key: true,
            input_device: String::new(),
            output_device: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ShortcutSettings {
    pub workspace: String,
    pub voice: String,
}

impl Default for ShortcutSettings {
    fn default() -> Self {
        // Ctrl+Alt+Space is commonly taken (IME switchers, launchers).
        ShortcutSettings { workspace: "Ctrl+Alt+J".into(), voice: "Ctrl+Alt+K".into() }
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
    /// Folders the user works in; used to resolve "my project".
    pub project_roots: Vec<String>,
}

impl Default for AgentSettings {
    fn default() -> Self {
        AgentSettings { max_steps: 24, confirm_moderate: true, project_roots: Vec::new() }
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub provider: ProviderSettings,
    pub voice: VoiceSettings,
    pub persona: PersonaSettings,
    pub shortcuts: ShortcutSettings,
    pub privacy: PrivacySettings,
    pub agent: AgentSettings,
    pub launch_at_startup: bool,
    pub reduce_motion: bool,
}
