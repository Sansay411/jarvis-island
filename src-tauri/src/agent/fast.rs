//! Conservative local commands: match the whole utterance or use the model.
use crate::ai::types::ToolCall;
use serde_json::{json, Value};

pub fn answer(tool:&str,summary:&str,data:&Value)->String {
    if tool=="system_volume" {
        if data.get("muted").and_then(Value::as_bool)==Some(true) {return "Звук выключен".into();}
        if let Some(volume)=data.get("volume").and_then(Value::as_u64) {return format!("Громкость {volume}%");}
    }
    summary.into()
}

pub fn command(text: &str) -> Option<ToolCall> {
    let lower = text.trim().to_lowercase().replace('ё', "е");
    let question = lower.ends_with('?');
    let mut s = lower.trim_end_matches(['.', '!', '?']).trim();
    let mut addressed=s;
    for _ in 0..3 {
        if let Some(rest)=["эй, ","эй ","слушай, ","слушай ","а ","ну "].iter().find_map(|lead|addressed.strip_prefix(lead)) {addressed=rest.trim();} else {break;}
    }
    for name in ["джарвис","джарвиз","джарвес","джарес","джарис","джар","jarvis","javis"] {
        if let Some(rest)=addressed.strip_prefix(name).filter(|rest|rest.starts_with([',',' ','.',':'])) {
            s=rest.trim_start_matches([',',' ','.',':']).trim();break;
        }
    }
    if let Some(rest) = s.strip_prefix("пожалуйста, ") { s = rest; }
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
        "пауза" | "поставь на паузу" => Some(("media_control", json!({"action":"pause"}))),
        "продолжи воспроизведение" | "включи музыку" | "продолжи музыку" => Some(("media_control", json!({"action":"play"}))),
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
    None
}

/// «Открой телеграм», «запусти хром»: one short app name and nothing else.
/// Files, sites, folders and anything compound go to the model; a name the
/// app index does not know fails and the model recovers from that.
fn app_to_open(s: &str) -> Option<String> {
    let rest = ["открой ", "запусти ", "включи приложение ", "открой приложение "].iter().find_map(|p| s.strip_prefix(p))?;
    let rest = rest.trim().trim_end_matches(", пожалуйста").trim_end_matches(" пожалуйста").trim();
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
        ] {
            let call = command(text).unwrap_or_else(|| panic!("not matched: {text}"));
            assert_eq!(call.name, tool);
            assert_eq!(serde_json::from_str::<Value>(&call.arguments).unwrap(), args);
        }
    }

    #[test]
    fn ambiguous_negated_or_compound_requests_always_go_to_model() {
        for text in ["не выключай звук", "как выключить звук?", "пауза и открой браузер", "громкость 30 и яркость 40", "громкость 150", "громкость -5", "скажи: выключи звук", "открой калькулятор и посчитай 2+2", "яркость 60?", "выключи звук?", "громкость 30, пожалуйста, и открой почту", "открой последний файл", "открой папку загрузки", "открой сайт ютуб", "открой хром и найди погоду", "открой мне это"] {
            assert!(command(text).is_none(), "must go to model: {text}");
        }
    }
}
