//! Conservative local commands: match the whole utterance or use the model.
use crate::ai::types::ToolCall;
use serde_json::{json, Value};

pub fn answer(tool:&str,args:&str,summary:&str,data:&Value)->String {
    if tool=="reminder_cancel" { return "Таймер отменён".into(); }
    if tool=="system_volume" {
        if data.get("muted").and_then(Value::as_bool)==Some(true) {return "Звук выключен".into();}
        if let Some(volume)=data.get("volume").and_then(Value::as_u64) {return format!("Громкость {volume}%");}
    }
    if tool=="media_control" {
        let action = serde_json::from_str::<Value>(args).ok().and_then(|a| a.get("action").and_then(Value::as_str).map(str::to_string)).unwrap_or_default();
        return media_answer(&action, data);
    }
    summary.into()
}

/// «Играет «Self Aware» — Moonshine»: what the player does now, said the way
/// a person would. The tool's own summary is written for the model.
fn media_answer(action: &str, data: &Value) -> String {
    let state = data.get("state").unwrap_or(data);
    let text = |k: &str| state.get(k).and_then(Value::as_str).unwrap_or("").trim().to_string();
    let (title, artist) = (text("title"), text("artist"));
    let title: String = if title.chars().count() > 70 { format!("{}…", title.chars().take(70).collect::<String>()) } else { title };
    let track = match (title.is_empty(), artist.is_empty()) {
        (true, _) => None,
        (false, true) => Some(format!("«{title}»")),
        (false, false) => Some(format!("«{title}» — {artist}")),
    };
    let available = state.get("available").and_then(Value::as_bool).unwrap_or(false);
    match (action, track) {
        ("pause" | "stop", _) => "Пауза.".into(),
        ("status", _) if !available => "Сейчас ничего не играет.".into(),
        ("status", Some(t)) if text("status") == "paused" => format!("На паузе {t}."),
        ("play" | "play_pause" | "status", Some(t)) => format!("Играет {t}."),
        ("play" | "play_pause" | "status", None) => "Играет.".into(),
        ("next" | "previous", Some(t)) => format!("{t}."),
        ("next", None) => "Следующий трек.".into(),
        ("previous", None) => "Предыдущий трек.".into(),
        _ => "Готово.".into(),
    }
}

/// A media command that did not go through, in one short sentence. The model
/// has nothing to add: it would only open a music site nobody asked for.
pub fn media_failure(data: &Value) -> String {
    let available = data.get("state").and_then(|s| s.get("available")).and_then(Value::as_bool).unwrap_or(false);
    if available { "Плеер не отреагировал.".into() } else { "Плеер не открыт — запусти музыку в своём плеере.".into() }
}

/// What a whole utterance asks for when no model is needed.
pub enum Fast {
    Call(ToolCall),
    /// Small talk: answered at once, nothing to do.
    Say(&'static str),
}

/// How the recogniser hears the name (seen live: «Жервис», «Дарвис»).
const NAMES: &[&str] = &["джарвис", "джарвиз", "джарвес", "джервис", "жарвис", "жервис", "дарвис", "джарес", "джарис", "джар", "jarvis", "javis"];

/// The utterance without greetings, the name (said once or twice, at
/// either end) and «пожалуйста» anywhere: «Джарвис. Жервис. Открой,
/// пожалуйста, телеграм» → «открой телеграм».
fn bare(text: &str) -> String {
    let lower = text.trim().to_lowercase().replace('ё', "е");
    let mut s: String = lower.trim_end_matches(['.', '!', '?']).trim().to_string();
    for _ in 0..4 {
        let before = s.clone();
        if let Some(rest) = ["эй, ", "эй ", "слушай, ", "слушай ", "а ", "ну "].iter().find_map(|lead| s.strip_prefix(lead)) {
            s = rest.trim().to_string();
        }
        // The same ear for the name as the voice wake-up (`voice::wake`).
        let first = s.split([',', ' ', '.', ':', '!']).next().unwrap_or("").to_string();
        let last = s.rsplit([',', ' ']).next().unwrap_or("").to_string();
        if !first.is_empty() && (NAMES.contains(&first.as_str()) || crate::voice::wake::is_name(&first)) {
            s = s[first.len()..].trim_start_matches([',', ' ', '.', ':', '!']).trim().to_string();
        } else if !last.is_empty() && last.len() < s.len() && (NAMES.contains(&last.as_str()) || crate::voice::wake::is_name(&last)) {
            s = s[..s.len() - last.len()].trim_end_matches([',', ' ']).trim().to_string();
        }
        if s == before {
            break;
        }
    }
    let s = s.replace(", пожалуйста,", "").replace(", пожалуйста", "").replace("пожалуйста, ", "").replace(" пожалуйста", "");
    s.trim().trim_end_matches(['.', '!', ',']).trim().to_string()
}

/// Thanks and greetings, the whole utterance only.
fn small_talk(s: &str) -> Option<&'static str> {
    let s = s.trim_end_matches(" брат").trim_end_matches(" бро").trim_end_matches([' ', ',']);
    Some(match s {
        "спасибо" | "спасибо большое" | "большое спасибо" | "благодарю" | "от души" | "спс" | "пасиб" | "спасибо тебе" => "Обращайтесь.",
        "отлично" | "супер" | "круто" | "класс" | "молодец" | "красава" | "умница" | "отлично сработано" => "Рад стараться.",
        "привет" | "здравствуй" | "здравствуйте" | "доброе утро" | "добрый день" | "добрый вечер" => "Здравствуйте. Чем помочь?",
        "как дела" | "как ты" | "как жизнь" | "как сам" => "Всё работает штатно. Чем займёмся?",
        "ты тут" | "ты здесь" | "ты меня слышишь" | "слышишь меня" => "Да, слушаю.",
        _ => return None,
    })
}

/// The face that goes with a small-talk reply (see `store/emote.ts`).
pub fn emote_for(reply: &str) -> &'static str {
    match reply {
        "Обращайтесь." => "wink",
        "Рад стараться." => "heart",
        "Да, слушаю." => "surprised",
        _ => "laugh",
    }
}

/// Sites said by name: opened directly instead of looking for an app.
fn site(s: &str) -> Option<&'static str> {
    Some(match s {
        "ютуб" | "ютюб" | "youtube" | "ютубчик" => "https://www.youtube.com",
        "яндекс музыку" | "яндекс музыка" | "яндекс.музыку" | "музыку яндекс" => "https://music.yandex.ru",
        "вк" | "вконтакте" | "в контакте" | "вэка" => "https://vk.com",
        "гугл" | "google" => "https://www.google.com",
        "яндекс" => "https://ya.ru",
        "кинопоиск" => "https://www.kinopoisk.ru",
        "гитхаб" | "github" | "гит хаб" => "https://github.com",
        "чат гпт" | "чатгпт" | "chatgpt" | "chat gpt" => "https://chatgpt.com",
        "переводчик" | "гугл переводчик" => "https://translate.google.com",
        "карты" | "яндекс карты" => "https://yandex.ru/maps",
        "авито" => "https://www.avito.ru",
        "озон" => "https://www.ozon.ru",
        "вайлдберриз" | "вайлдбериз" | "wildberries" => "https://www.wildberries.ru",
        _ => return None,
    })
}

/// Windows Settings pages.
fn settings_page(s: &str) -> Option<&'static str> {
    let page = s.strip_prefix("настройки ").or_else(|| s.strip_prefix("параметры "))?;
    Some(match page {
        "звука" | "звук" => "ms-settings:sound",
        "экрана" | "дисплея" | "монитора" => "ms-settings:display",
        "блютуз" | "bluetooth" | "блютуза" => "ms-settings:bluetooth",
        "сети" | "интернета" | "вайфай" | "wi-fi" | "вай фай" => "ms-settings:network",
        "приложений" => "ms-settings:appsfeatures",
        "питания" | "батареи" => "ms-settings:powersleep",
        "обновлений" | "обновления" => "ms-settings:windowsupdate",
        "мыши" => "ms-settings:mousetouchpad",
        "клавиатуры" | "языка" => "ms-settings:keyboard",
        _ => return None,
    })
}

pub fn command(text: &str) -> Option<Fast> {
    command_with(text, crate::platform::media::has_player())
}

/// `has_player`: a player is open, so a bare «дальше» or «назад» is about it.
fn command_with(text: &str, has_player: bool) -> Option<Fast> {
    let question = text.trim().ends_with('?');
    let owned = bare(text);
    let s = owned.as_str();
    if let Some(reply) = small_talk(s) {
        return Some(Fast::Say(reply));
    }
    if let Some(action) = media(s, has_player).filter(|_| !question) {
        return Some(Fast::Call(call("media_control", json!({ "action": action }))));
    }
    if media_status(s) {
        return Some(Fast::Call(call("media_control", json!({ "action": "status" }))));
    }
    tool_command(s, question).map(Fast::Call)
}

/// The player action a whole utterance asks for, if that is all it asks
/// («Джарвис, дальше», «поставь на паузу», «верни музыку»). The voice gate
/// uses it too: an exact media command is short and unmistakable, unlike the
/// sentences a song or a film says.
pub fn media_action(text: &str) -> Option<&'static str> {
    if text.trim().ends_with('?') {
        return None;
    }
    media(bare(text).as_str(), crate::platform::media::has_player())
}

/// An exact device command (player, volume, brightness): it runs locally, so
/// the voice gate may take it without the name right after a reply.
pub fn device_command(text: &str) -> bool {
    match command(text) {
        Some(Fast::Call(c)) => matches!(c.name.as_str(), "media_control" | "system_volume" | "display_brightness"),
        _ => false,
    }
}

/// Words that ask for the next or the previous track only while a player is
/// open: with nothing playing, «дальше» more likely means «go on».
const NEEDS_PLAYER: &[&str] = &[
    "дальше", "давай дальше", "дальше давай", "назад", "следующий", "следующая", "следующую", "следующее", "предыдущий",
    "предыдущая", "предыдущую", "предыдущее", "переключи", "пропусти",
];

fn media(s: &str, has_player: bool) -> Option<&'static str> {
    let s = s.trim_end_matches([' ', ',', '.', '!']);
    if NEEDS_PLAYER.contains(&s) && !has_player {
        return None;
    }
    Some(match s {
        "играй" | "играй дальше" | "играй музыку" | "музыку играй" | "продолжай играть" | "пусть играет" | "включи музыку"
        | "включи музыку обратно" | "включи обратно" | "включай музыку" | "включи музычку" | "верни музыку" | "верни музыку обратно"
        | "давай музыку" | "музыку давай" | "музыку" | "музыка" | "плей" | "play" | "воспроизведи" | "продолжи воспроизведение"
        | "возобнови" | "возобнови музыку" | "возобнови воспроизведение" | "продолжи музыку" | "продолжай музыку" | "сними с паузы"
        | "сними паузу" | "убери паузу" | "сними музыку с паузы" | "сними видео с паузы" | "продолжи видео" => "play",
        "пауза" | "на паузу" | "паузу" | "поставь на паузу" | "поставь паузу" | "поставь музыку на паузу" | "поставь видео на паузу"
        | "поставь трек на паузу" | "поставь песню на паузу" | "останови музыку" | "останови видео" | "останови трек"
        | "останови песню" | "останови воспроизведение" | "выключи музыку" | "выключи песню" | "стоп музыка" | "стоп музыку"
        | "музыку стоп" | "музыка стоп" | "стоп песня" | "стоп видео" | "хватит музыки" | "приостанови" | "приостанови музыку"
        | "приостанови воспроизведение" | "тормозни музыку" | "pause" => "pause",
        "дальше" | "давай дальше" | "дальше давай" | "следующий" | "следующая" | "следующую" | "следующее" | "следующий трек"
        | "следующая песня" | "следующую песню" | "следующее видео" | "переключи" | "переключи трек" | "переключи песню"
        | "другую песню" | "другой трек" | "скип" | "скипни" | "пропусти" | "пропусти трек" | "пропусти песню" | "некст" | "next"
        | "трек дальше" | "песню дальше" | "включи следующую" | "включи следующий" | "включи следующую песню"
        | "включи следующий трек" => "next",
        "назад" | "трек назад" | "песню назад" | "предыдущий" | "предыдущая" | "предыдущую" | "предыдущее" | "предыдущий трек"
        | "предыдущая песня" | "предыдущую песню" | "предыдущее видео" | "прошлый трек" | "прошлую песню" | "прошлая песня"
        | "верни предыдущий" | "верни предыдущую" | "верни прошлую" | "верни прошлый трек" | "верни предыдущий трек"
        | "верни предыдущую песню" | "верни прошлую песню" | "включи предыдущую" | "включи предыдущий"
        | "включи предыдущую песню" | "включи предыдущий трек" | "previous" => "previous",
        _ => return None,
    })
}

/// «Что играет?», «что это за песня?»: read the player; a question is fine.
fn media_status(s: &str) -> bool {
    matches!(
        s,
        "что играет" | "что сейчас играет" | "что за песня" | "что это за песня" | "что за трек" | "что это за трек" | "что это играет"
            | "как называется песня" | "как называется трек" | "как называется эта песня" | "кто поет" | "кто это поет"
            | "кто исполняет" | "какая песня играет" | "какой трек играет" | "какая это песня" | "какая сейчас песня"
    )
}

fn tool_command(s: &str, question: bool) -> Option<ToolCall> {
    // The stopwatch and timers by voice: every word is about them, nothing
    // else can be meant.
    let clock = match s {
        "запусти секундомер" | "включи секундомер" | "старт секундомер" | "секундомер" | "засеки время" | "продолжи секундомер" => Some(("stopwatch", json!({"action":"start"}))),
        "останови секундомер" | "стоп секундомер" | "выключи секундомер" | "пауза секундомер" | "поставь секундомер на паузу" => Some(("stopwatch", json!({"action":"pause"}))),
        "сбрось секундомер" | "обнули секундомер" => Some(("stopwatch", json!({"action":"reset"}))),
        "круг" | "отметь круг" | "новый круг" | "отсечка" => Some(("stopwatch", json!({"action":"lap"}))),
        "сколько прошло" | "сколько на секундомере" | "сколько времени прошло" => Some(("stopwatch", json!({"action":"status"}))),
        "сколько осталось" | "сколько осталось на таймере" | "сколько до конца таймера" | "сколько еще осталось" => Some(("reminders_list", json!({}))),
        "отмени таймер" | "выключи таймер" | "останови таймер" | "сбрось таймер" | "убери таймер" => Some(("reminder_cancel", json!({}))),
        "стоп фокус" | "останови фокус" | "выключи фокус" | "отмени фокус" | "сбрось фокус" => Some(("focus_session", json!({"action":"cancel"}))),
        "пауза фокуса" | "фокус на паузу" | "поставь фокус на паузу" | "приостанови фокус" => Some(("focus_session", json!({"action":"pause"}))),
        "продолжи фокус" | "возобнови фокус" | "сними фокус с паузы" => Some(("focus_session", json!({"action":"resume"}))),
        "заверши фокус" | "фокус готов" => Some(("focus_session", json!({"action":"complete"}))),
        "статус фокуса" | "сколько осталось фокуса" => Some(("focus_session", json!({"action":"status"}))),
        _ => None,
    };
    if let Some((name, args)) = clock { return Some(call(name, args)); }
    let read_only = match s {
        "какая громкость" | "какая сейчас громкость" | "покажи громкость" | "проверь громкость" => Some(("system_volume",json!({"action":"get"}))),
        "какая яркость" | "покажи яркость" => Some(("display_brightness",json!({}))),
        "сколько заряда у наушников" | "сколько заряда в наушниках" | "сколько заряда на наушниках" | "какой заряд у наушников"
        | "какой заряд наушников" | "заряд наушников" | "сколько процентов у наушников" | "сколько процентов на наушниках"
        | "сколько процентов в наушниках" | "батарея наушников" | "сколько зарядки у наушников" | "сколько зарядки в наушниках"
        | "проверь заряд наушников" | "покажи заряд наушников" | "сколько заряда у эйрподсов" | "заряд эйрподсов"
        | "сколько заряда у airpods" | "заряд airpods" => Some(("bluetooth_battery", json!({}))),
        _ => None,
    };
    if let Some((name, args)) = read_only { return Some(call(name, args)); }
    // A question about an action is not permission to perform it.
    if question { return None; }
    let exact = match s {
        "выключи звук" | "отключи звук" => Some(("system_volume", json!({"action":"mute"}))),
        "включи звук" => Some(("system_volume", json!({"action":"unmute"}))),
        "сделай потише" | "потише" | "сделай тише" | "тише" | "еще тише" | "звук тише" | "убавь звук" | "убавь громкость" => Some(("system_volume",json!({"action":"change","delta":-10}))),
        "сделай погромче" | "погромче" | "сделай громче" | "громче" | "еще громче" | "звук громче" | "прибавь звук" | "прибавь громкость" => Some(("system_volume",json!({"action":"change","delta":10}))),
        "громкость на максимум" | "максимальная громкость" | "громкость на полную" | "сделай на полную" => Some(("system_volume", json!({"action":"set","percent":100}))),
        "сделай намного тише" | "намного тише" | "сильно тише" => Some(("system_volume", json!({"action":"change","delta":-25}))),
        "сделай намного громче" | "намного громче" | "сильно громче" => Some(("system_volume", json!({"action":"change","delta":25}))),
        "сделай чуть тише" | "сделай чуть потише" | "сделай немного потише" | "чуть тише" | "чуть потише" | "немного тише" | "немного потише" => Some(("system_volume",json!({"action":"change","delta":-5}))),
        "сделай чуть громче" | "сделай чуть погромче" | "сделай немного погромче" | "чуть громче" | "чуть погромче" | "немного громче" | "немного погромче" => Some(("system_volume",json!({"action":"change","delta":5}))),
        "открой калькулятор" => Some(("app_launch", json!({"query":"Калькулятор"}))),
        "открой блокнот" => Some(("app_launch", json!({"query":"Блокнот"}))),
        _ => None,
    };
    if let Some((name, args)) = exact { return Some(call(name, args)); }
    if matches!(s, "включи фокус" | "запусти фокус" | "режим фокуса") {
        return Some(call("focus_session", json!({"action":"start","minutes":25})));
    }
    for p in ["включи фокус на ", "запусти фокус на ", "поставь фокус на ", "режим фокуса на ", "фокус на ", "фокус "] {
        if let Some((secs, tail)) = s.strip_prefix(p).and_then(crate::reminders::parse_duration) {
            if tail.is_empty() {
                let mins = ((secs + 59) / 60).clamp(1, 240);
                return Some(call("focus_session", json!({"action":"start","minutes":mins})));
            }
        }
    }
    for p in ["поставь таймер на ", "поставь таймер ", "заведи таймер на ", "таймер на ", "таймер ", "засеки "] {
        if let Some((secs, tail)) = s.strip_prefix(p).and_then(crate::reminders::parse_duration) {
            if tail.is_empty() {
                return Some(call("reminder_create", json!({"kind":"timer","in_seconds":secs})));
            }
        }
    }
    for p in ["напомни мне через ", "напомни через "] {
        if let Some((secs, tail)) = s.strip_prefix(p).and_then(crate::reminders::parse_duration) {
            let text = tail.strip_prefix("про ").or_else(|| tail.strip_prefix("о ")).unwrap_or(&tail).trim().to_string();
            if !text.is_empty() && !text.contains(" и ") {
                return Some(call("reminder_create", json!({"kind":"reminder","in_seconds":secs,"text":text})));
            }
        }
    }
    if let Some(rest) = ["открой ", "зайди на ", "зайди в ", "перейди на ", "открой сайт "].iter().find_map(|p| s.strip_prefix(p)) {
        if let Some(url) = site(rest.trim()).or_else(|| settings_page(rest.trim())) {
            return Some(call("open_target", json!({"target": url})));
        }
    }
    if let Some(app) = app_to_open(s) { return Some(call("app_launch", json!({"query": app}))); }
    for (prefix, tool) in [
        ("поставь громкость на ","system_volume"), ("установи громкость ","system_volume"), ("громкость ","system_volume"),
        ("поставь яркость на ","display_brightness"), ("установи яркость ","display_brightness"), ("яркость ","display_brightness"),
    ] {
        if let Some(value) = s.strip_prefix(prefix) {
            let n = percent(value)?;
            let args = if tool == "system_volume" { json!({"action":"set","percent":n}) } else { json!({"percent":n}) };
            return Some(call(tool, args));
        }
    }
    for (prefixes, sign) in [(["громче на ", "погромче на ", "прибавь на ", "прибавь громкость на "], 1i64), (["тише на ", "потише на ", "убавь на ", "убавь громкость на "], -1)] {
        if let Some(value) = prefixes.iter().find_map(|p| s.strip_prefix(p)) {
            let n = percent(value)?;
            return Some(call("system_volume", json!({"action":"change","delta":sign * n as i64})));
        }
    }
    None
}

/// «Открой телеграм», «запусти хром»: one short app name and nothing else.
/// Files, sites, folders and anything compound go to the model; a name the
/// app index does not know fails and the model recovers from that.
fn app_to_open(s: &str) -> Option<String> {
    let rest = ["открой ", "запусти ", "включи приложение ", "открой приложение "].iter().find_map(|p| s.strip_prefix(p))?;
    let rest = rest.trim();
    let words: Vec<&str> = rest.split_whitespace().collect();
    if words.is_empty() || words.len() > 3 || rest.contains([',', ':', ';', '"', '«', '/', '\\', '.']) {
        return None;
    }
    const NOT_APPS: &[&str] = &[
        "и", "файл", "файлы", "папку", "папка", "сайт", "страницу", "ссылку", "вкладку", "документ", "последний", "последнюю",
        "последнее", "мне", "его", "ее", "это", "этот", "эту", "там", "тут", "новую", "новый", "окно", "почту", "письмо", "чат",
        "проект", "музыку", "видео", "фото", "картинку", "настройки", "диск", "загрузки", "документы",
    ];
    if words.iter().any(|w| NOT_APPS.contains(w)) {
        return None;
    }
    Some(rest.to_string())
}

fn call(name: &str, args: Value) -> ToolCall {
    ToolCall { id:uuid::Uuid::new_v4().to_string(), name:name.into(), arguments:args.to_string() }
}

fn percent(s: &str) -> Option<u8> {
    let mut s = s.trim();
    for suffix in [" процентов", " процента", " процент", "%"] {
        if let Some(rest) = s.strip_suffix(suffix) { s = rest.trim(); break; }
    }
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()) {
        return s.parse::<u8>().ok().filter(|n| *n <= 100);
    }
    let small = ["ноль","один","два","три","четыре","пять","шесть","семь","восемь","девять","десять","одиннадцать","двенадцать","тринадцать","четырнадцать","пятнадцать","шестнадцать","семнадцать","восемнадцать","девятнадцать"];
    if let Some(n) = small.iter().position(|word| *word == s) { return Some(n as u8); }
    if s == "сто" { return Some(100); }
    let tens = ["двадцать","тридцать","сорок","пятьдесят","шестьдесят","семьдесят","восемьдесят","девяносто"];
    for (i, word) in tens.iter().enumerate() {
        let base = (i as u8 + 2) * 10;
        if s == *word { return Some(base); }
        if let Some(rest) = s.strip_prefix(word).and_then(|r| r.strip_prefix(' ')) {
            return small[1..10].iter().position(|w| *w == rest).map(|n| base + n as u8 + 1);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    #[test]
    fn fast_volume_answer_speaks_verified_value_without_reading_hardware_name() {
        assert_eq!(answer("system_volume","{}","Громкость 24% (Realtek Audio)",&json!({"volume":24,"muted":false})),"Громкость 24%");
        assert_eq!(answer("system_volume","{}","Звук выключен (Realtek Audio)",&json!({"volume":24,"muted":true})),"Звук выключен");
        assert_eq!(answer("app_launch","{}","Калькулятор открыт",&json!({})),"Калькулятор открыт");
    }

    #[test]
    fn media_answers_name_the_track_like_a_person() {
        let st = |title: &str, artist: &str, status: &str| json!({"state":{"available":true,"title":title,"artist":artist,"status":status}});
        let a = |action: &str, data: &Value| answer("media_control", &json!({"action":action}).to_string(), "Следующий трек: …", data);
        assert_eq!(a("next", &st("Self Aware", "Moonshine", "playing")), "«Self Aware» — Moonshine.");
        assert_eq!(a("play", &st("Про любовь", "Земфира", "playing")), "Играет «Про любовь» — Земфира.");
        assert_eq!(a("pause", &st("Про любовь", "Земфира", "paused")), "Пауза.");
        assert_eq!(a("status", &st("Про любовь", "", "paused")), "На паузе «Про любовь».");
        assert_eq!(a("status", &json!({"state":{"available":false}})), "Сейчас ничего не играет.");
        assert_eq!(a("previous", &st("", "", "playing")), "Предыдущий трек.");
        assert_eq!(media_failure(&json!({"state":{"available":false}})), "Плеер не открыт — запусти музыку в своём плеере.");
    }

    fn media_of(text: &str, has_player: bool) -> Option<String> {
        match command_with(text, has_player) {
            Some(Fast::Call(c)) if c.name == "media_control" => {
                serde_json::from_str::<Value>(&c.arguments).unwrap()["action"].as_str().map(str::to_string)
            }
            _ => None,
        }
    }

    #[test]
    fn every_way_to_say_play_pause_next_back_is_a_local_media_command() {
        for (text, action) in [
            ("Джарвис, играй.", "play"), ("Играй дальше", "play"), ("Верни музыку", "play"), ("Включи музыку обратно.", "play"),
            ("Музыку!", "play"), ("Сними с паузы", "play"), ("Жарвис, продолжи музыку, пожалуйста", "play"),
            ("Пауза.", "pause"), ("Джарвис, пауза!", "pause"), ("Поставь на паузу", "pause"), ("Стоп музыка", "pause"),
            ("Джарвис, стоп музыка.", "pause"), ("Выключи музыку", "pause"), ("Останови видео", "pause"),
            ("Жарвис. Дальше.", "next"), ("Дальше", "next"), ("Следующий трек", "next"), ("Переключи песню", "next"),
            ("Скипни", "next"), ("Ярвис, следующую", "next"), ("Пропусти трек", "next"),
            ("Назад", "previous"), ("Джарвис, назад.", "previous"), ("Предыдущий трек", "previous"), ("Верни прошлую песню", "previous"),
            ("Что сейчас играет?", "status"), ("Джарвис, что это за песня?", "status"), ("Кто поёт?", "status"),
        ] {
            assert_eq!(media_of(text, true).as_deref(), Some(action), "{text}");
        }
    }

    #[test]
    fn bare_next_and_back_need_an_open_player_and_media_needs_the_whole_utterance() {
        // Nothing open: «дальше» more likely means «go on», «назад» — «back».
        for text in ["Дальше", "Назад", "Следующий"] {
            assert_eq!(media_of(text, false), None, "{text}");
        }
        // Unambiguous forms still work without a known player (the tool then says so).
        assert_eq!(media_of("Следующий трек", false).as_deref(), Some("next"));
        assert_eq!(media_of("Пауза", false).as_deref(), Some("pause"));
        // A question about an action, a negation or more words: the model decides.
        for text in [
            "Дальше?", "Не ставь на паузу", "Пауза и открой браузер", "Включи музыку на ютубе", "Назад в браузере",
            "Дальше рассказывай", "Включи музыку погромче", "Играй что-нибудь весёлое", "Что дальше?", "Музыку Земфиры",
        ] {
            assert_eq!(media_of(text, true), None, "{text}");
        }
    }

    #[test]
    fn exact_commands_work_without_model_including_spoken_numbers() {
        for (text, tool, args) in [
            ("Какая сейчас громкость?", "system_volume", json!({"action":"get"})),
            ("Эй, Джарвис, какая сейчас громкость?", "system_volume", json!({"action":"get"})),
            ("Джарвис, громкость двадцать пять процентов.", "system_volume", json!({"action":"set","percent":25})),
            ("Поставь громкость на 30%", "system_volume", json!({"action":"set","percent":30})),
            ("Выключи звук", "system_volume", json!({"action":"mute"})),
            ("Яркость 60", "display_brightness", json!({"percent":60})),
            ("Пауза", "media_control", json!({"action":"pause"})),
            ("Открой калькулятор", "app_launch", json!({"query":"Калькулятор"})),
            ("Сделай потише", "system_volume", json!({"action":"change","delta":-10})),
            ("Сделай погромче", "system_volume", json!({"action":"change","delta":10})),
            ("Включи музыку", "media_control", json!({"action":"play"})),
            ("Открой телеграм", "app_launch", json!({"query":"телеграм"})),
            ("Джарвис, запусти Visual Studio Code.", "app_launch", json!({"query":"visual studio code"})),
            ("Убавь звук", "system_volume", json!({"action":"change","delta":-10})),
            ("Следующая песня", "media_control", json!({"action":"next"})),
            ("Поставь таймер на 5 минут", "reminder_create", json!({"kind":"timer","in_seconds":300})),
            ("Джарвис, таймер на полчаса.", "reminder_create", json!({"kind":"timer","in_seconds":1800})),
            ("Напомни через 20 минут позвонить маме", "reminder_create", json!({"kind":"reminder","in_seconds":1200,"text":"позвонить маме"})),
            ("Джарвис. Жервис. Открой антигравити.", "app_launch", json!({"query":"антигравити"})),
            ("Включи, пожалуйста, музыку.", "media_control", json!({"action":"play"})),
            ("Открой, пожалуйста, ютуб, Джарвис", "open_target", json!({"target":"https://www.youtube.com"})),
            ("Зайди на кинопоиск", "open_target", json!({"target":"https://www.kinopoisk.ru"})),
            ("Открой настройки звука", "open_target", json!({"target":"ms-settings:sound"})),
            ("Останови музыку", "media_control", json!({"action":"pause"})),
            ("Громче на 20", "system_volume", json!({"action":"change","delta":20})),
            ("Сделай чуть потише", "system_volume", json!({"action":"change","delta":-5})),
            ("Громкость на максимум", "system_volume", json!({"action":"set","percent":100})),
            ("Джарвис, запусти секундомер", "stopwatch", json!({"action":"start"})),
            ("Стоп секундомер", "stopwatch", json!({"action":"pause"})),
            ("Сколько прошло?", "stopwatch", json!({"action":"status"})),
            ("Сколько осталось?", "reminders_list", json!({})),
            ("Отмени таймер", "reminder_cancel", json!({})),
            ("Фокус на 25 минут", "focus_session", json!({"action":"start","minutes":25})),
            ("Джарвис, фокус на сорок пять минут.", "focus_session", json!({"action":"start","minutes":45})),
            ("Стоп фокус", "focus_session", json!({"action":"cancel"})),
            ("Пауза фокуса", "focus_session", json!({"action":"pause"})),
            ("Продолжи фокус", "focus_session", json!({"action":"resume"})),
        ] {
            let Some(Fast::Call(call)) = command(text) else { panic!("not matched: {text}") };
            assert_eq!(call.name, tool);
            assert_eq!(serde_json::from_str::<Value>(&call.arguments).unwrap(), args);
        }
    }

    #[test]
    fn small_talk_is_answered_without_model() {
        for text in ["Спасибо, Жервис.", "От души, брат.", "А Джарвис, как у тебя дела?", "Привет", "Джарвис, ты тут?"] {
            let reply = match command(text) { Some(Fast::Say(r)) => Some(r), _ => None };
            if text.contains("у тебя") {
                assert!(reply.is_none(), "«как у тебя дела» is not in the list: {text}");
            } else {
                assert!(reply.is_some(), "not answered: {text}");
            }
        }
    }

    #[test]
    fn ambiguous_negated_or_compound_requests_always_go_to_model() {
        for text in ["не выключай звук", "как выключить звук?", "пауза и открой браузер", "громкость 30 и яркость 40", "громкость 150", "громкость -5", "скажи: выключи звук", "открой калькулятор и посчитай 2+2", "яркость 60?", "выключи звук?", "громкость 30, пожалуйста, и открой почту", "открой последний файл", "открой папку загрузки", "открой сайт ютуб", "открой хром и найди погоду", "открой мне это", "спасибо, а теперь открой почту", "открой ютуб и включи музыку", "громче на 200", "открой настройки чего-нибудь"] {
            assert!(command(text).is_none(), "must go to model: {text}");
        }
    }
}
