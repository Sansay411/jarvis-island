//! Home Assistant through its MCP server (Settings → «Подключения» →
//! Home Assistant). The server lends its Assist tools (`HassTurnOn`,
//! `HassLightSet`, `GetLiveContext`…, namespaced as `intent__HassTurnOn`,
//! `homeassistant__GetLiveContext` since 2026). On top of them JARVIS keeps
//! a snapshot of the home (devices, rooms, states) from `GetLiveContext`:
//! the device map goes into the system prompt, so the model knows the exact
//! names without a lookup; what is on right now goes into each request's
//! context, so «поярче» or «верни как было» need no extra step. The
//! snapshot refreshes in the background and right after every change.
//!
//! Risk: Home Assistant marks every control intent as destructive, which
//! would ask before each light. Here a light or a socket is low risk;
//! locks, alarms, valves and garage doors are always confirmed.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::tools::Risk;

/// The live snapshot is too old to be told as «now» after this.
const STALE_MIN: i64 = 30;
const MAP_BUDGET: usize = 7000;
const NOW_BUDGET: usize = 1200;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Entity {
    /// The name first, then the aliases set in Home Assistant.
    pub names: Vec<String>,
    pub domain: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// The room first, then its aliases.
    #[serde(default)]
    pub areas: Vec<String>,
    #[serde(default)]
    pub attrs: BTreeMap<String, String>,
}

impl Entity {
    pub fn name(&self) -> &str {
        self.names.first().map(String::as_str).unwrap_or("")
    }

    fn attr(&self, key: &str) -> Option<&str> {
        self.attrs.get(key).map(String::as_str).map(str::trim).filter(|v| !v.is_empty() && *v != "None")
    }

    /// Opening it lets someone in or floods something: always confirmed.
    fn sensitive(&self) -> bool {
        matches!(self.domain.as_str(), "lock" | "alarm_control_panel" | "valve")
            || (self.domain == "cover" && matches!(self.attr("device_class"), Some("garage" | "gate" | "door")))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    /// The MCP server id (part of the tool names).
    pub server: String,
    pub server_name: String,
    /// RFC 3339, UTC.
    pub at: String,
    pub entities: Vec<Entity>,
}

/// `intent__HassTurnOn` → `HassTurnOn`; plain names stay as they are.
pub fn base(tool: &str) -> &str {
    tool.rsplit("__").next().unwrap_or(tool)
}

/// One of Home Assistant's own tools (Assist API).
pub fn is_home_tool(base: &str) -> bool {
    base == "GetLiveContext" || base.starts_with("Hass")
}

/// A tool that changes something in the home.
pub fn is_control(base: &str) -> bool {
    base.starts_with("Hass") && !base.starts_with("HassGet") && !base.ends_with("TimerStatus")
}

fn norm(s: &str) -> String {
    crate::learn::norm(s)
}

// ---------- parsing ----------

/// The text of a `GetLiveContext` result: `{"result": "Live Context: …"}`
/// (older versions add `"success": true`), or the text itself.
pub fn result_text(text: &str) -> Option<String> {
    match serde_json::from_str::<Value>(text.trim()) {
        Ok(v) => v.get("result").and_then(Value::as_str).map(str::to_string),
        Err(_) => (!text.trim().is_empty()).then(|| text.to_string()),
    }
}

/// `key: value` with an identifier key.
fn split_kv(t: &str) -> Option<(String, String)> {
    let colon = t.find(':')?;
    let key = &t[..colon];
    if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    let rest = &t[colon + 1..];
    if !rest.is_empty() && !rest.starts_with(' ') {
        return None;
    }
    Some((key.to_string(), rest.trim().to_string()))
}

fn unquote(raw: &str) -> String {
    let t = raw.trim();
    if t.len() >= 2 && t.starts_with('\'') && t.ends_with('\'') {
        return t[1..t.len() - 1].replace("''", "'");
    }
    if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
        return t[1..t.len() - 1].replace("\\\"", "\"").replace("\\\\", "\\");
    }
    t.to_string()
}

fn list_of(raw: &str) -> Vec<String> {
    unquote(raw).split(", ").map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect()
}

#[derive(Default)]
struct RawItem {
    fields: Vec<(String, String)>,
    attrs: Vec<(String, String)>,
    in_attrs: bool,
    last_attr: bool,
}

/// The YAML list Home Assistant writes for its live (and static) context:
/// `- names: …` items with `domain`, `state`, `areas` and an `attributes`
/// mapping; long values may wrap onto indented lines.
pub fn parse_live_context(text: &str) -> Vec<Entity> {
    let mut items: Vec<RawItem> = Vec::new();
    let mut item_indent: Option<usize> = None;
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let t = line.trim_start_matches(' ');
        let indent = line.len() - t.len();
        if let Some(rest) = t.strip_prefix("- ") {
            if item_indent.is_none_or(|i| i == indent) {
                item_indent = Some(indent);
                let mut item = RawItem::default();
                if let Some(kv) = split_kv(rest) {
                    item.fields.push(kv);
                }
                items.push(item);
                continue;
            }
        }
        let (Some(ii), Some(cur)) = (item_indent, items.last_mut()) else { continue };
        let key_indent = ii + 2;
        if indent == key_indent {
            if let Some((k, v)) = split_kv(t) {
                cur.in_attrs = k == "attributes" && v.is_empty();
                cur.last_attr = false;
                if !cur.in_attrs {
                    cur.fields.push((k, v));
                }
                continue;
            }
        }
        if indent == key_indent + 2 && cur.in_attrs {
            if let Some(kv) = split_kv(t) {
                cur.attrs.push(kv);
                cur.last_attr = true;
                continue;
            }
        }
        if indent > key_indent {
            let target = if cur.last_attr { cur.attrs.last_mut() } else { cur.fields.last_mut() };
            if let Some((_, v)) = target {
                v.push(' ');
                v.push_str(t.trim());
            }
        }
    }
    items
        .into_iter()
        .filter_map(|raw| {
            let get = |k: &str| raw.fields.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_str());
            let names = list_of(get("names").or_else(|| get("name"))?);
            let domain = unquote(get("domain")?);
            if names.is_empty() || domain.is_empty() {
                return None;
            }
            Some(Entity {
                names,
                domain,
                state: get("state").map(unquote),
                areas: get("areas").map(list_of).unwrap_or_default(),
                attrs: raw.attrs.iter().map(|(k, v)| (k.clone(), unquote(v))).collect(),
            })
        })
        .collect()
}

// ---------- prompt ----------

fn domain_rank(domain: &str) -> u8 {
    match domain {
        "light" => 0,
        "switch" => 1,
        "climate" => 2,
        "cover" => 3,
        "fan" => 4,
        "media_player" => 5,
        "lock" => 6,
        "vacuum" => 7,
        "scene" => 8,
        "script" => 9,
        "humidifier" | "water_heater" | "valve" | "input_boolean" => 10,
        "button" | "input_button" | "select" | "number" => 11,
        "alarm_control_panel" => 12,
        "sensor" => 20,
        "binary_sensor" => 21,
        _ => 30,
    }
}

fn entity_label(e: &Entity) -> String {
    let mut s = e.name().to_string();
    let aliases: Vec<String> = e.names.iter().skip(1).take(2).map(|a| format!("«{a}»")).collect();
    if !aliases.is_empty() {
        s.push_str(&format!(" (также {})", aliases.join(", ")));
    }
    let mut tag = e.domain.clone();
    if matches!(e.domain.as_str(), "sensor" | "binary_sensor" | "cover") {
        if let Some(dc) = e.attr("device_class") {
            tag.push_str(&format!(": {dc}"));
        }
    }
    s.push_str(&format!(" [{tag}]"));
    s
}

/// Devices by room, for the system prompt (no states: they change, the map
/// does not, so the provider's prompt cache keeps serving it).
pub fn map_block(snap: &Snapshot) -> String {
    let mut rooms: Vec<(String, Vec<String>, Vec<&Entity>)> = Vec::new();
    let mut ordered: Vec<&Entity> = snap.entities.iter().collect();
    ordered.sort_by(|a, b| domain_rank(&a.domain).cmp(&domain_rank(&b.domain)).then_with(|| a.name().cmp(b.name())));
    for e in ordered {
        let room = e.areas.first().cloned().unwrap_or_default();
        match rooms.iter_mut().find(|(r, ..)| *r == room) {
            Some((_, _, list)) => list.push(e),
            None => rooms.push((room, e.areas.iter().skip(1).cloned().collect(), vec![e])),
        }
    }
    rooms.sort_by(|a, b| (a.0.is_empty(), &a.0).cmp(&(b.0.is_empty(), &b.0)));
    let mut out = format!(
        "\nThe home («{}», {} devices; names are exact — pass them as `name`, rooms as `area`):",
        snap.server_name,
        snap.entities.len()
    );
    let mut omitted = 0;
    for (room, aliases, list) in rooms {
        let title = if room.is_empty() {
            "Без комнаты".to_string()
        } else if aliases.is_empty() {
            room
        } else {
            format!("{room} (также {})", aliases.iter().map(|a| format!("«{a}»")).collect::<Vec<_>>().join(", "))
        };
        let mut line = format!("\n- {title}: ");
        let mut first = true;
        for e in list {
            let label = entity_label(e);
            if out.len() + line.len() + label.len() > MAP_BUDGET {
                omitted += 1;
                continue;
            }
            if !first {
                line.push_str(", ");
            }
            line.push_str(&label);
            first = false;
        }
        if !first {
            out.push_str(&line);
        }
    }
    if omitted > 0 {
        out.push_str(&format!("\n- …и ещё {omitted} (найдёшь через GetLiveContext)"));
    }
    out
}

fn pct(e: &Entity, key: &str) -> String {
    e.attr(key).map(|v| format!(" {v}%")).unwrap_or_default()
}

/// What is on, open or playing now, for the request context; nothing when
/// the snapshot is stale or has no states.
pub fn now_block(snap: &Snapshot, now: DateTime<Utc>) -> Option<String> {
    let at = DateTime::parse_from_rfc3339(&snap.at).ok()?.with_timezone(&Utc);
    let age = (now - at).num_minutes().max(0);
    if age > STALE_MIN || snap.entities.iter().all(|e| e.state.is_none()) {
        return None;
    }
    let (mut on, mut open, mut climate, mut media, mut locks, mut sensors) = (vec![], vec![], vec![], vec![], vec![], vec![]);
    let mut unavailable = 0;
    for e in &snap.entities {
        let Some(st) = e.state.as_deref() else { continue };
        let name = e.name();
        if matches!(st, "unavailable" | "unknown") {
            if domain_rank(&e.domain) < 20 {
                unavailable += 1;
            }
            continue;
        }
        match e.domain.as_str() {
            "light" if st == "on" => on.push(format!("{name}{}", pct(e, "brightness_pct"))),
            "fan" if st == "on" => on.push(format!("{name}{}", pct(e, "percentage"))),
            "switch" | "input_boolean" | "humidifier" if st == "on" => on.push(name.to_string()),
            "vacuum" if matches!(st, "cleaning" | "returning") => on.push(format!("{name} ({st})")),
            "cover" if matches!(st, "open" | "opening") => open.push(format!("{name}{}", pct(e, "current_position"))),
            "climate" if st != "off" => {
                let target = e.attr("temperature").map(|t| format!(" → {t}°")).unwrap_or_default();
                let current = e.attr("current_temperature").map(|t| format!(" (сейчас {t}°)")).unwrap_or_default();
                climate.push(format!("{name} {st}{target}{current}"));
            }
            "media_player" if !matches!(st, "off" | "standby" | "idle") => {
                let title = e.attr("media_title").map(|t| format!(" «{t}»")).unwrap_or_default();
                media.push(format!("{name} {st}{title}"));
            }
            "lock" if st != "locked" => locks.push(format!("{name} {st}")),
            "alarm_control_panel" => locks.push(format!("{name} {st}")),
            "sensor" if matches!(e.attr("device_class"), Some("temperature" | "humidity")) && sensors.len() < 6 => {
                sensors.push(format!("{name} {st}{}", e.attr("unit_of_measurement").map(|u| format!(" {u}")).unwrap_or_default()));
            }
            _ => {}
        }
    }
    let mut parts = Vec::new();
    for (label, list) in [("on", on), ("open", open), ("climate", climate), ("playing", media), ("locks/alarm", locks)] {
        if !list.is_empty() {
            parts.push(format!("{label} — {}", list.join(", ")));
        }
    }
    if parts.is_empty() {
        parts.push("all lights and devices are off".into());
    }
    if !sensors.is_empty() {
        parts.push(format!("sensors — {}", sensors.join(", ")));
    }
    if unavailable > 0 {
        parts.push(format!("unavailable devices: {unavailable}"));
    }
    let ago = if age == 0 { "just now".to_string() } else { format!("{age} min ago") };
    let mut line = format!("- Home now (Home Assistant, {ago}): {}\n", parts.join("; "));
    if line.len() > NOW_BUDGET {
        let mut cut = NOW_BUDGET;
        while !line.is_char_boundary(cut) {
            cut -= 1;
        }
        line.truncate(cut);
        line.push_str("…\n");
    }
    Some(line)
}

/// How to run the home well; added to the system prompt with the map.
pub const GUIDE: &str = r#"

Smart home — you run the user's home through Home Assistant (the mcp__…Hass* tools and GetLiveContext). Be precise and quick, like a good butler who knows the house:
- Targets: a device → `name` with its exact name from the map (add `domain` when names repeat); a whole room → `area` + `domain`; everything of a kind → `domain` only («выключи весь свет» → HassTurnOff domain ["light"]). Words → domains: свет/люстра/лампа/ночник/подсветка → light; розетка/чайник/обогреватель → usually switch; кондиционер/отопление/батарея/термостат → climate; шторы/жалюзи/рольставни/ворота → cover; телевизор/колонка → media_player; вентилятор → fan; пылесос → vacuum; замок → lock.
- Understand from half a word: the user rarely says the exact name — «люстра», «большой свет», «у меня», «в зале» — map it to the closest device or room in the map by meaning (synonyms, Russian↔English, transliteration, the user's own words from your lessons). With no target named: the device from the last request in this conversation, then the user's habit at this hour (context), then the room they are usually in. Ask only when two devices are equally likely and the wrong one matters.
- Several devices in one request → all the calls in one step (they run in parallel). Never call GetLiveContext before a plain on/off of a device from the map.
- Relative requests use the current state («Home now» in the context; GetLiveContext when you need fresher data or a sensor value): «поярче» → +25 % from the current brightness, «потусклее» → −25 %, «потеплее/похолоднее» about the air → ±1 °C from the target temperature, «ещё» → the same step again, «верни как было» → the state before your last change in this conversation.
- Lights: HassLightSet with `brightness` 0–100 and/or `color` as an English colour name (red, blue, purple, orange, pink, white…); «тёплый/холодный свет» is `temperature` in kelvin (тёплый ≈ 2700, нейтральный ≈ 4000, холодный ≈ 6000). Climate: HassClimateSetTemperature `temperature` in °C. Covers: HassSetPosition `position` 0–100 (0 closed), HassTurnOn opens, HassTurnOff closes. Media: HassMediaPause / HassMediaUnpause / HassMediaNext / HassSetVolume (`volume_level` 0–100). Scenes: HassTurnOn with the scene name. A lock: HassTurnOn locks, HassTurnOff unlocks.
- Moods and scenarios by meaning: «я спать / спокойной ночи» → lights off (leave a night light only if the user asked), media paused; «я ушёл» → lights and media off; «кино» → main light off or dimmed to 10–20 %, a warm accent light on; «мне холодно/жарко» → the climate device of the room up/down by 1–2 °C, or say what you could do. Prefer the user's own scenes and scripts when one fits.
- Questions about the home («горит свет на кухне?», «сколько градусов в спальне?», «что включено?») → answer from «Home now» if it covers it, otherwise GetLiveContext with a filter (`area`, `domain` or `name`).
- After acting, one short line in your manner with the verified result («Кухня в темноте.», «Спальня — двадцать два, как любишь.»). If Home Assistant answers that no device matched, look it up with GetLiveContext and retry once with the exact name; then teach yourself the user's word with `learn` kind=alias.
- Locks, alarms, valves and garage doors are confirmed by the app automatically; just call the tool."#;

// ---------- risk ----------

fn strings(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::String(s)) => vec![s.to_lowercase()],
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).map(str::to_lowercase).collect(),
        _ => Vec::new(),
    }
}

/// Words that mean a lock, a door or an alarm (when the snapshot can't say).
const SENSITIVE_WORDS: &[&str] = &["замок", "замк", "двер", "ворот", "гараж", "сигнализ", "охран", "калитк", "кран", "lock", "door", "gate", "garage", "alarm", "valve"];

pub fn find_entity<'a>(snap: &'a Snapshot, name: &str) -> Option<&'a Entity> {
    let n = norm(name);
    snap.entities.iter().find(|e| e.names.iter().any(|x| norm(x) == n))
}

fn in_area<'a>(snap: &'a Snapshot, area: &str) -> impl Iterator<Item = &'a Entity> + 'a {
    let a = norm(area);
    snap.entities.iter().filter(move |e| e.areas.iter().any(|x| norm(x) == a))
}

/// Does the call reach a lock, an alarm, a valve or a garage door?
fn sensitive_target(args: &Value, snap: Option<&Snapshot>) -> bool {
    let domains = strings(args.get("domain"));
    if domains.iter().any(|d| matches!(d.as_str(), "lock" | "alarm_control_panel" | "valve")) {
        return true;
    }
    if strings(args.get("device_class")).iter().any(|d| matches!(d.as_str(), "garage" | "gate" | "door")) {
        return true;
    }
    let name = args.get("name").and_then(Value::as_str).unwrap_or("").trim();
    let area = args.get("area").and_then(Value::as_str).unwrap_or("").trim();
    if let Some(snap) = snap {
        if !name.is_empty() {
            if let Some(e) = find_entity(snap, name) {
                return e.sensitive();
            }
        }
        // A whole room (or the whole home) with no domain may include its lock.
        if name.is_empty() && domains.is_empty() {
            if area.is_empty() {
                return snap.entities.iter().any(Entity::sensitive);
            }
            return in_area(snap, area).any(Entity::sensitive);
        }
    }
    let lower = name.to_lowercase();
    !name.is_empty() && SENSITIVE_WORDS.iter().any(|w| lower.contains(w))
}

const READ_ONLY: &[&str] = &[
    "GetLiveContext",
    "GetDateTime",
    "render_template",
    "search_actions",
    "search",
    "get_history",
    "get_config_entries",
    "get_entries",
    "get_logs",
    "get_system_info",
    "search_integrations",
];

/// Action domains safe to call without asking (lights, sockets, media…).
const SAFE_ACTIONS: &[&str] = &[
    "light", "switch", "fan", "media_player", "climate", "humidifier", "input_boolean", "scene", "vacuum", "water_heater", "remote", "input_number", "input_select",
    "select", "number",
];
/// Action domains that restart, unlock, notify people or change the system.
const DANGEROUS_ACTIONS: &[&str] = &[
    "homeassistant", "hassio", "lock", "alarm_control_panel", "valve", "backup", "recorder", "update", "notify", "persistent_notification", "system_log", "zha",
    "zwave_js", "mqtt", "shell_command", "rest_command", "python_script", "automation", "script",
];

/// The risk of one Home Assistant tool call, or `None` for tools this
/// module does not know (the caller then asks once per conversation).
pub fn risk(base: &str, args: &Value, snap: Option<&Snapshot>) -> Option<Risk> {
    if READ_ONLY.contains(&base) || base.starts_with("HassGet") || base.ends_with("TimerStatus") {
        return Some(Risk::Low);
    }
    if base.starts_with("Hass") {
        if sensitive_target(args, snap) {
            return Some(Risk::High);
        }
        let has = |k: &str| match args.get(k) {
            Some(Value::String(s)) => !s.trim().is_empty(),
            Some(Value::Array(a)) => !a.is_empty(),
            _ => false,
        };
        let targeted = ["name", "area", "floor", "domain", "device_class"].iter().any(|k| has(k));
        if matches!(base, "HassTurnOn" | "HassTurnOff") && !targeted {
            return Some(Risk::Moderate);
        }
        return Some(Risk::Low);
    }
    if base == "call_action" {
        let action = args.get("action").and_then(Value::as_str).unwrap_or("").trim().to_lowercase();
        let domain = action.split('.').next().unwrap_or("");
        let entity_domains: Vec<String> = strings(args.get("target").and_then(|t| t.get("entity_id")))
            .iter()
            .filter_map(|id| id.split('.').next().map(str::to_string))
            .collect();
        if DANGEROUS_ACTIONS.contains(&domain) || entity_domains.iter().any(|d| DANGEROUS_ACTIONS.contains(&d.as_str())) {
            return Some(Risk::High);
        }
        if domain == "cover" {
            let ids = strings(args.get("target").and_then(|t| t.get("entity_id")));
            let garage = ids.iter().any(|id| SENSITIVE_WORDS.iter().any(|w| id.contains(w)));
            return Some(if garage { Risk::High } else { Risk::Low });
        }
        return Some(if SAFE_ACTIONS.contains(&domain) { Risk::Low } else { Risk::Moderate });
    }
    None
}

/// The argument a learned word stands in for: a device name or a room.
pub fn alias_key(snap: &Snapshot, value: &str) -> Option<&'static str> {
    if find_entity(snap, value).is_some() {
        return Some("name");
    }
    in_area(snap, value).next().map(|_| "area")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const LIVE: &str = "Live Context: An overview of the areas and the devices in this smart home:\n\
- names: Люстра, Большой свет\n  domain: light\n  state: 'on'\n  areas: Гостиная, Зал\n  attributes:\n    brightness: '178'\n    brightness_pct: '70'\n\
- names: Ночник\n  domain: light\n  state: 'off'\n  areas: Спальня\n\
- names: Входная дверь\n  domain: lock\n  state: unlocked\n  areas: Прихожая\n\
- names: Кондиционер спальни\n  domain: climate\n  state: cool\n  areas: Спальня\n  attributes:\n    current_temperature: '24.5'\n    temperature: '22'\n\
- names: Температура спальни\n  domain: sensor\n  state: '23.4'\n  areas: Спальня\n  attributes:\n    device_class: temperature\n    unit_of_measurement: °C\n\
- names: 'Очень длинное имя: розетка у дивана с подсветкой и\n    зарядкой'\n  domain: switch\n  state: 'off'\n\
- names: Ворота\n  domain: cover\n  state: closed\n  areas: Двор\n  attributes:\n    device_class: gate\n";

    fn snap() -> Snapshot {
        Snapshot { server: "ha".into(), server_name: "Home Assistant".into(), at: Utc::now().to_rfc3339(), entities: parse_live_context(LIVE) }
    }

    #[test]
    fn live_context_yaml_becomes_entities() {
        let e = parse_live_context(LIVE);
        assert_eq!(e.len(), 7);
        assert_eq!(e[0].names, vec!["Люстра", "Большой свет"]);
        assert_eq!(e[0].state.as_deref(), Some("on"));
        assert_eq!(e[0].areas, vec!["Гостиная", "Зал"]);
        assert_eq!(e[0].attrs.get("brightness_pct").map(String::as_str), Some("70"));
        assert_eq!(e[2].state.as_deref(), Some("unlocked"));
        assert_eq!(e[4].attrs.get("unit_of_measurement").map(String::as_str), Some("°C"));
        // A quoted name that wrapped onto the next line.
        assert_eq!(e[5].name(), "Очень длинное имя: розетка у дивана с подсветкой и зарядкой");
        assert!(e[5].areas.is_empty());
        assert_eq!(result_text(r#"{"success": true, "result": "Live Context: x"}"#).as_deref(), Some("Live Context: x"));
        assert_eq!(result_text(r#"{"error": "no entities"}"#), None);
    }

    #[test]
    fn the_map_lists_rooms_with_exact_names_and_no_states() {
        let map = map_block(&snap());
        assert!(map.contains("- Гостиная (также «Зал»): Люстра (также «Большой свет») [light]"), "{map}");
        assert!(map.contains("Ворота [cover: gate]"), "{map}");
        assert!(map.contains("- Без комнаты: Очень длинное имя"), "{map}");
        assert!(!map.contains("unlocked") && !map.contains("70"), "{map}");
    }

    #[test]
    fn now_tells_what_is_on_and_skips_stale_data() {
        let s = snap();
        let now = now_block(&s, Utc::now()).unwrap();
        assert!(now.contains("on — Люстра 70%"), "{now}");
        assert!(now.contains("climate — Кондиционер спальни cool → 22° (сейчас 24.5°)"), "{now}");
        assert!(now.contains("locks/alarm — Входная дверь unlocked"), "{now}");
        assert!(now.contains("sensors — Температура спальни 23.4 °C"), "{now}");
        assert!(!now.contains("Ночник"), "{now}");
        assert_eq!(now_block(&s, Utc::now() + chrono::Duration::minutes(45)), None);
    }

    #[test]
    fn lights_are_low_risk_and_locks_doors_and_alarms_are_high() {
        let s = snap();
        let r = |base: &str, args: Value| risk(base, &args, Some(&s));
        assert_eq!(r("HassTurnOn", json!({"name": "Люстра"})), Some(Risk::Low));
        assert_eq!(r("HassLightSet", json!({"area": "Спальня", "brightness": 30})), Some(Risk::Low));
        assert_eq!(r("HassTurnOff", json!({"domain": ["light"]})), Some(Risk::Low));
        assert_eq!(r("HassTurnOff", json!({"name": "Входная дверь"})), Some(Risk::High));
        assert_eq!(r("HassTurnOn", json!({"name": "Ворота"})), Some(Risk::High));
        assert_eq!(r("HassTurnOff", json!({"area": "Прихожая"})), Some(Risk::High));
        assert_eq!(r("HassTurnOff", json!({"area": "Спальня"})), Some(Risk::Low));
        assert_eq!(r("HassTurnOff", json!({"domain": "lock"})), Some(Risk::High));
        assert_eq!(r("GetLiveContext", json!({})), Some(Risk::Low));
        assert_eq!(r("call_action", json!({"action": "light.turn_on", "target": {"area_id": "kitchen"}})), Some(Risk::Low));
        assert_eq!(r("call_action", json!({"action": "homeassistant.restart"})), Some(Risk::High));
        assert_eq!(r("call_action", json!({"action": "switch.turn_off", "target": {"entity_id": "lock.front"}})), Some(Risk::High));
        assert_eq!(r("call_action", json!({"action": "input_text.set_value"})), Some(Risk::Moderate));
        assert_eq!(r("some_script", json!({})), None);
        // Without a snapshot the words decide.
        assert_eq!(risk("HassTurnOff", &json!({"name": "Замок на двери"}), None), Some(Risk::High));
        assert_eq!(risk("HassTurnOn", &json!({"name": "Свет"}), None), Some(Risk::Low));
        assert_eq!(risk("HassTurnOff", &json!({}), None), Some(Risk::Moderate));
    }

    #[test]
    fn tool_names_and_alias_targets() {
        assert_eq!(base("intent__HassTurnOn"), "HassTurnOn");
        assert_eq!(base("HassTurnOn"), "HassTurnOn");
        assert!(is_home_tool("GetLiveContext") && is_control("HassLightSet") && !is_control("HassGetState"));
        let s = snap();
        assert_eq!(alias_key(&s, "большой свет"), Some("name"));
        assert_eq!(alias_key(&s, "Зал"), Some("area"));
        assert_eq!(alias_key(&s, "Телевизор"), None);
    }
}
