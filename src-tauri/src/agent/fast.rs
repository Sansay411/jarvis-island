//! Conservative local commands: match the whole utterance or use the model.
use crate::ai::types::ToolCall;
use serde_json::{json, Value};

pub fn answer(tool:&str,summary:&str,data:&Value)->String {
    if tool=="reminder_cancel" { return "Таймер отменён".into(); }
    if tool=="system_volume" {
        if data.get("muted").and_then(Value::as_bool)==Some(true) {return "Звук выключен".into();}
        if let Some(volume)=data.get("volume").and_then(Value::as_u64) {return format!("Громкость {volume}%");}
    }
    summary.into()
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
        for name in NAMES {
            if let Some(rest) = s.strip_prefix(name).filter(|rest| rest.is_empty() || rest.starts_with([',', ' ', '.', ':', '!'])) {
                s = rest.trim_start_matches([',', ' ', '.', ':', '!']).trim().to_string();
                break;
            }
            if let Some(rest) = s.strip_suffix(name).filter(|rest| rest.ends_with([',', ' '])) {
                s = rest.trim_end_matches([',', ' ']).trim().to_string();
                break;
            }
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
    let question = text.trim().ends_with('?');
    let owned = bare(text);
    let s = owned.as_str();
    if let Some(reply) = small_talk(s) {
        return Some(Fast::Say(reply));
    }
    tool_command(s, question).map(Fast::Call)
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
        _ => None,
    };
    if let Some((name, args)) = clock { return Some(call(name, args)); }
    let read_only = match s {
        "какая громкость" | "какая сейчас громкость" | "покажи громкость" | "проверь громкость" => Some(("system_volume",json!({"action":"get"}))),
        "какая яркость" | "покажи яркость" => Some(("display_brightness",json!({}))),
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
        "пауза" | "поставь на паузу" | "поставь музыку на паузу" | "останови музыку" | "выключи музыку" | "стоп музыка" | "останови видео" | "поставь видео на паузу" => Some(("media_control", json!({"action":"pause"}))),
        "громкость на максимум" | "максимальная громкость" | "громкость на полную" | "сделай на полную" => Some(("system_volume", json!({"action":"set","percent":100}))),
        "сделай намного тише" | "намного тише" | "сильно тише" => Some(("system_volume", json!({"action":"change","delta":-25}))),
        "сделай намного громче" | "намного громче" | "сильно громче" => Some(("system_volume", json!({"action":"change","delta":25}))),
        "сделай чуть тише" | "сделай чуть потише" | "сделай немного потише" | "чуть тише" | "чуть потише" | "немного тише" | "немного потише" => Some(("system_volume",json!({"action":"change","delta":-5}))),
        "сделай чуть громче" | "сделай чуть погромче" | "сделай немного погромче" | "чуть громче" | "чуть погромче" | "немного громче" | "немного погромче" => Some(("system_volume",json!({"action":"change","delta":5}))),
        "продолжи воспроизведение" | "включи музыку" | "продолжи музыку" | "играй дальше" | "сними с паузы" | "сними паузу" => Some(("media_control", json!({"action":"play"}))),
        "следующий трек" | "следующая песня" | "переключи трек" | "следующую песню" => Some(("media_control", json!({"action":"next"}))),
        "предыдущий трек" | "предыдущая песня" | "предыдущую песню" => Some(("media_control", json!({"action":"previous"}))),
        "открой калькулятор" => Some(("app_launch", json!({"query":"Калькулятор"}))),
        "открой блокнот" => Some(("app_launch", json!({"query":"Блокнот"}))),
        _ => None,
    };
    if let Some((name, args)) = exact { return Some(call(name, args)); }
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
        assert_eq!(answer("system_volume","Громкость 24% (Realtek Audio)",&json!({"volume":24,"muted":false})),"Громкость 24%");
        assert_eq!(answer("system_volume","Звук выключен (Realtek Audio)",&json!({"volume":24,"muted":true})),"Звук выключен");
        assert_eq!(answer("app_launch","Калькулятор открыт",&json!({})),"Калькулятор открыт");
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
