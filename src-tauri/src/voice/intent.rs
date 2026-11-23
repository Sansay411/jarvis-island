//! Whom a recognised phrase is for, while JARVIS is awake.
//!
//! The microphone hears the whole room: a film, a partner, the user talking
//! to themselves. Live use on 2026-11-01 showed what happens when every
//! audible phrase is a command: remarks like «Не могу найти. Два месяца
//! осталось» were glued to a running task, restarting it again and again
//! with garbage, while «Стоп, стоп, стоп» and the name itself did not stop
//! anything. So while JARVIS works or speaks, only the user's own signals
//! count: a stop word, the name, or the rest of a thought they had just
//! begun. Everything else is the room.
//!
//! Right after an answer the user often goes on without the name — but so
//! does the room: on 2026-11-20/21 a song, a game and a talk at home were
//! answered for minutes on end («Вачем замуж», «Это моя», «Алиса, нежно-белый
//! цвет» — that one ran shell commands). So an unnamed phrase counts only if
//! it is built like words to JARVIS (a request, a question to «ты», an answer
//! to its own question, an exact device command), never while a song or a film
//! plays, and even then the model hears it with a note and may stay silent.

use std::time::Duration;

use super::wake::{self, LEADS};

/// Words that cut JARVIS off. GigaAM writes a lone «стоп» in Latin
/// («Stop.») — seen live.
pub(super) const STOP_WORDS: &[&str] = &[
    "стоп", "хватит", "отмена", "отмени", "замолчи", "тихо", "стой", "stop", "остановись", "прекрати", "перестань", "достаточно",
];

/// A thought is «the rest of what they were saying» only this long after the
/// turn started; later it is a new remark.
const MERGE_WINDOW: Duration = Duration::from_secs(6);

/// How far into a remark the name may stand for the words from it on to be
/// the command («Он сказал сначала… А Джарвис, открывай…»).
const NAME_WITHIN_BUSY: usize = 4;
const NAME_WITHIN_IDLE: usize = 12;

/// What to do with a phrase.
#[derive(Debug, PartialEq, Eq)]
pub enum Intent {
    /// «Стоп», «хватит»: silence, cancel the task, end the conversation.
    Stop,
    /// The name (or an interruption) with nothing to do yet: stop what is
    /// going on and listen.
    Call,
    /// «Всё, спасибо», «отбой», «можешь отдыхать»: say this (nothing when
    /// empty) and wait for the name again.
    Dismiss(&'static str),
    /// Cancel the running turn or reply and handle these words instead.
    Replace(String),
    /// An ordinary request (or the rest of one); the words to use.
    Request(String),
    /// Unnamed words built like words to JARVIS: the model hears them with a
    /// note that they may be the room's and may answer with silence. `cut`:
    /// they came over a running task or a spoken answer.
    Ambient { text: String, cut: bool },
    /// Not for JARVIS.
    Ignore,
}

#[derive(Default)]
pub struct Context {
    /// A voice turn runs or JARVIS is speaking.
    pub busy: bool,
    /// JARVIS is reading out a reply right now (paused by this very phrase
    /// or still queued). Over a spoken answer the user is in a conversation:
    /// a phrase built like an interruption («Нет, давай короче», «Подожди,
    /// открой проводник», «А какая погода завтра?») steers it without the name.
    /// Silent work keeps the strict rule — the room must not steer a task.
    pub speaking: bool,
    /// The agent is working on a request (not only speaking).
    pub turn_active: bool,
    /// The phrase began right after the previous one closed.
    pub continues: bool,
    /// How long the running turn has been going.
    pub turn_age: Duration,
    /// No echo of JARVIS in the microphone.
    pub headphones: bool,
    /// A player is playing: songs and films say sentences too.
    pub media_playing: bool,
    /// The user's own media command was carried out moments ago: «дальше…
    /// дальше…» goes on without the name.
    pub media_chain: bool,
    /// JARVIS's last answer ended with a question.
    pub asked: bool,
    /// JARVIS asked for the command («Слушаю») or the user opened the
    /// conversation themselves: the words count as addressed.
    pub invited: bool,
}

/// Stop words (any number, with greetings and the name) and what follows.
pub(super) enum StopRest<'a> {
    /// Nothing else was said.
    Alone,
    /// A command follows: stop and do this.
    Command(&'a str),
    /// A remark follows («Стоп, не то», «Стоп, спасибо»): just stop.
    Remark,
}

/// What follows a stop word that opens a phrase when it is a remark and not
/// a command: «Стоп, не то», «Стоп, спасибо».
const REMARK_STARTS: &[&str] = &["не", "нет", "это", "то", "так", "ладно", "ну", "всё", "все", "хорошо", "спасибо", "ой", "блин", "да", "я", "мы"];

/// Leading stop words, greetings and the name, then what remains.
pub(super) fn stop_prefix(text: &str) -> Option<StopRest<'_>> {
    let w = wake::spans(text);
    let (mut stopped, mut named, mut i) = (false, false, 0);
    while let Some((_, word)) = w.get(i) {
        let word = word.as_str();
        if STOP_WORDS.contains(&word) {
            stopped = true;
        } else if wake::is_name(word) {
            named = true;
        } else if !LEADS.contains(&word) {
            break;
        }
        i += 1;
    }
    if !stopped {
        return None;
    }
    let Some((at, first)) = w.get(i) else { return Some(StopRest::Alone) };
    let rest = w.len() - i;
    let command = named || rest >= 3 || (rest == 2 && !REMARK_STARTS.contains(&first.as_str()));
    Some(if command { StopRest::Command(&text[*at..]) } else { StopRest::Remark })
}

/// «Стоп», «Стоп, стоп, стоп!», «Джар, хватит» — and nothing else.
pub fn is_stop(text: &str) -> bool {
    matches!(stop_prefix(text), Some(StopRest::Alone))
}

/// A player command that counts from anyone, without the name and with the
/// conversation closed (the user's choice, 2026-11-21: «пусть на все
/// голоса»). Returns the command to run: an exact player phrase of three
/// words at most, or a bare «стоп»/«хватит». Several in one breath
/// («Стоп, стоп, стоп.», «Пауза. Играй. Верни музыку.» — heard live) count
/// when every one of them is a player command: the last is what the user
/// wants. Not «музыка!» alone (an exclamation, not an order), and not «тихо»
/// or «отмена» — said to people far more often than to a player.
pub fn player_command(text: &str) -> Option<String> {
    let parts: Vec<&str> = text.split(['.', ',', '!', ';']).map(str::trim).filter(|p| p.chars().any(char::is_alphabetic)).collect();
    if parts.is_empty() || parts.len() > 4 || text.trim_end().ends_with('?') {
        return None;
    }
    let one = |p: &str| {
        let w: Vec<String> = p.to_lowercase().split(|c: char| !c.is_alphabetic()).filter(|w| !w.is_empty()).map(str::to_string).collect();
        !w.is_empty()
            && w.len() <= 3
            && !matches!(w.as_slice(), [one] if one == "музыка" || one == "музыку")
            && (crate::agent::fast::media_action(p).is_some() || (w.len() <= 2 && w.iter().all(|w| matches!(w.as_str(), "стоп" | "stop" | "хватит"))))
    };
    if !parts.iter().all(|p| one(p)) {
        return None;
    }
    Some(if parts.len() == 1 { text.trim().to_string() } else { format!("{}.", parts[parts.len() - 1]) })
}

/// «…, Джарвис, хватит.», «… Стоп.»: the phrase ends by cutting JARVIS off
/// (heard live 2026-11-20: «Давай я тебе скину. Ничего, Жарвис, всё, Джарвис,
/// хватит. Стоп.» went to the model, which paused the music instead). The stop
/// words at the very end count with the name among them or as a sentence of
/// their own — «Он сказал: стоп» is a story, not a stop.
fn ends_with_stop(text: &str) -> bool {
    let w = wake::spans(text);
    let (mut stopped, mut named, mut i) = (false, false, w.len());
    while i > 0 {
        let word = w[i - 1].1.as_str();
        if STOP_WORDS.contains(&word) {
            stopped = true;
        } else if wake::is_name(word) {
            named = true;
        } else if !LEADS.contains(&word) {
            break;
        }
        i -= 1;
    }
    // The whole phrase is stop words: `stop_prefix` has it.
    if !stopped || i == 0 {
        return false;
    }
    named || text[..w[i].0].trim_end().ends_with(['.', '!', '?', '…'])
}

/// Other voice assistants by name: «Алиса, включи свет» is not for JARVIS.
const OTHER_ASSISTANTS: &[&str] = &["алиса", "алис", "алисочка", "маруся", "сири", "siri", "алекса", "alexa", "гугл", "google", "кортана"];

fn for_another_assistant(text: &str) -> bool {
    wake::spans(text).iter().map(|(_, w)| w.as_str()).find(|w| !LEADS.contains(w)).is_some_and(|w| OTHER_ASSISTANTS.contains(&w))
}

/// Words that close a conversation («всё, спасибо», «отбой», «можешь
/// отдыхать»), and the reply each kind gets.
const THANKS: &[&str] = &["спасибо", "спасибочки", "благодарю", "спс", "пасиб", "пасибо", "души"];
const PRAISE: &[&str] = &["молодец", "красава", "красавчик", "умница", "супер", "отлично", "круто", "класс"];
const ROUGH: &[&str] = &["отстань", "отвали", "нафиг", "нахрен", "нахер"];
const CLOSING: &[&str] = &[
    "все", "отбой", "свободен", "свободна", "пока", "покеда", "свидания", "связи", "отдыхай", "отдыхать", "выключайся",
    "выключаться", "отключайся", "пошел", "пошла", "ушел", "ничего",
];
/// Words that may go with them («ну всё, можешь идти отдыхать»).
const CLOSING_FILLER: &[&str] = &[
    "ну", "ладно", "хорошо", "окей", "ок", "ясно", "понятно", "да", "нет", "ага", "тогда", "уже", "можешь", "можно", "если", "ты", "на",
    "этом", "это", "больше", "не", "надо", "нужно", "до", "конец", "иди", "идти", "я", "брат", "бро", "от", "тебе", "большое", "огромное",
    "друг", "и", "а", "так", "вот", "сейчас", "пожалуйста", "спать",
];

/// «Всё, спасибо, Жар, если можешь выключаться»: the user is done.
fn dismissal(text: &str) -> Option<&'static str> {
    if text.trim_end().ends_with('?') {
        return None;
    }
    let words = wake::spans(text);
    if words.is_empty() || words.len() > 10 {
        return None;
    }
    // The warmest word sets the reply: «всё, спасибо» is a thank-you.
    let mut best: Option<(u8, &'static str)> = None;
    for (_, w) in &words {
        let w = w.as_str();
        let k = if THANKS.contains(&w) {
            (4, "Обращайтесь.")
        } else if PRAISE.contains(&w) {
            (3, "Рад стараться.")
        } else if CLOSING.contains(&w) {
            (2, "Хорошо.")
        } else if ROUGH.contains(&w) {
            (1, "Понял.")
        } else if CLOSING_FILLER.contains(&w) || LEADS.contains(&w) || wake::is_name(w) || wake::SHORT.contains(&w) {
            continue;
        } else {
            return None;
        };
        if best.is_none_or(|b| k.0 > b.0) {
            best = Some(k);
        }
    }
    best.map(|b| b.1)
}

/// Second-person words: a question with them is put to JARVIS.
const YOU: &[&str] = &["ты", "тебя", "тебе", "тобой", "твой", "твоя", "твое", "твои", "твоего", "твоей", "вы", "вас", "вам", "вами", "ваш", "ваша", "ваше", "ваши"];

/// Built like words to JARVIS: a request that opens with what to do, a real
/// question to it, «а завтра?» after an answer, or an answer to its own
/// question. Statements about the world («Это моя», «В процессе загрузки»)
/// are the room's.
fn looks_addressed(text: &str, asked: bool) -> bool {
    let all = plain_words(text);
    let lead = all.iter().take_while(|w| FILLER_LEADS.contains(&w.as_str()) || SOFT_STARTS.contains(&w.as_str())).count();
    let w = &all[lead..];
    let Some(first) = w.first().map(String::as_str) else { return false };
    let question = text.trim_end().ends_with('?');
    if VERBS.contains(&first) || (DIRECTIVES.contains(&first) && w.len() <= 3) {
        return true;
    }
    if question && w.len() >= 3 && (QUESTIONS.contains(&first) || w.iter().any(|x| YOU.contains(&x.as_str()))) {
        return true;
    }
    if question && all.first().is_some_and(|f| f == "а" || f == "и") && (2..=4).contains(&all.len()) {
        return true;
    }
    asked && all.len() <= 8
}

/// An unnamed phrase while JARVIS is idle (just after an answer, or after
/// «Слушаю» timed out).
fn follow_up(text: &str, ctx: &Context) -> Intent {
    // Closing words end the conversation; unnamed, silently — the room's
    // «ладно, я пошёл» must not get an answer. A cut name inside («Всё,
    // спасибо, Жар, …») is still the user talking to JARVIS.
    if let Some(reply) = dismissal(text) {
        let named = wake::spans(text).iter().any(|(_, w)| wake::is_name(w) || wake::SHORT.contains(&w.as_str()));
        return Intent::Dismiss(if named { reply } else { "" });
    }
    if (!ctx.media_playing || ctx.media_chain) && crate::agent::fast::device_command(text) {
        return Intent::Request(text.to_string());
    }
    // A song or a film says whole sentences: while one plays nothing else
    // counts without the name.
    if ctx.media_playing {
        return Intent::Ignore;
    }
    if looks_addressed(text, ctx.asked) {
        return Intent::Ambient { text: text.to_string(), cut: false };
    }
    Intent::Ignore
}

/// Decide what a recognised phrase means. `ctx.busy` is the heart of it:
/// while JARVIS works or speaks, the room must not steer it.
pub fn classify(text: &str, ctx: &Context) -> Intent {
    let take = |words: &str| {
        let words = words.to_string();
        if ctx.busy { Intent::Replace(words) } else { Intent::Request(words) }
    };
    if for_another_assistant(text) {
        return Intent::Ignore;
    }
    let addressed = wake::addressed(text).or(ctx.invited.then_some(wake::Addressed { only_name: false }));
    // An exact player command first: in «Стоп музыка» or «Джарвис, пауза» the
    // stop is the music's, not JARVIS's. Without the name it counts from
    // anyone, over a song or a film too (the user's choice, 2026-11-21) —
    // but not over JARVIS's own voice through the speakers (his reply may
    // say «пауза»), and not into a task he is working on.
    // Several in one breath: the last one (stop words go the stop way below).
    let player = player_command(text).filter(|c| !is_stop(c));
    if crate::agent::fast::media_action(text).is_some() || player.is_some() {
        let own = addressed.is_some() || stop_prefix(text).is_some();
        let anyone = (!ctx.busy && !ctx.media_playing) || (player.is_some() && (!ctx.busy || (ctx.headphones && !ctx.turn_active)));
        if own || ctx.media_chain || anyone {
            return take(player.as_deref().unwrap_or(text));
        }
        return Intent::Ignore;
    }
    match stop_prefix(text) {
        Some(StopRest::Alone | StopRest::Remark) => return Intent::Stop,
        Some(StopRest::Command(rest)) => return take(rest),
        None => {}
    }
    if ends_with_stop(text) {
        return Intent::Stop;
    }
    match addressed {
        Some(a) if a.only_name => return Intent::Call,
        Some(_) => return dismissal(text).map_or_else(|| take(text), Intent::Dismiss),
        None => {}
    }
    // The name after a few words of chatter: the command starts there.
    let within = if ctx.busy { NAME_WITHIN_BUSY } else { NAME_WITHIN_IDLE };
    if let Some(command) = wake::from_name(text, within) {
        return take(command);
    }
    if !ctx.busy {
        return follow_up(text, ctx);
    }
    // Unnamed words over a task or a reply: the user only if they go on with
    // the thought they had just begun.
    if ctx.continues && (!ctx.turn_active || ctx.turn_age < MERGE_WINDOW) {
        return Intent::Request(text.to_string());
    }
    // Over a song or a film only the user's own signals count: a stop word,
    // the name, «подожди».
    if ctx.media_playing {
        return if ctx.speaking && matches!(steering(text), Some(Steer::Hold)) { Intent::Call } else { Intent::Ignore };
    }
    // Headphones: no echo of JARVIS, but the room is still there — the
    // model judges plain words.
    if ctx.headphones && !ctx.turn_active {
        return Intent::Ambient { text: text.to_string(), cut: true };
    }
    // Talking over a spoken answer: an interruption has a recognisable
    // shape. Anything else is the room, and the paused reply goes on.
    if ctx.speaking {
        return match steering(text) {
            Some(Steer::Hold) => Intent::Call,
            Some(Steer::Words) => Intent::Ambient { text: text.to_string(), cut: true },
            None => Intent::Ignore,
        };
    }
    Intent::Ignore
}

/// «Подожди», «погоди» alone: stop talking and listen.
const HOLD_WORDS: &[&str] = &["подожди", "подождите", "погоди", "погодите", "постой", "постойте", "секунду", "секундочку", "минутку", "wait"];

/// One word that already says how to answer instead.
const DIRECTIVES: &[&str] = &[
    "короче", "покороче", "кратко", "подробнее", "поподробнее", "проще", "попроще", "медленнее", "помедленнее",
    "быстрее", "побыстрее", "громче", "погромче", "потише", "повтори", "заново", "сначала",
];

/// Openers of a correction or a new request; need words after them.
const STEER_STARTS: &[&str] = &["нет", "не", "давай", "давайте", "лучше", "вместо", "наоборот", "иначе"];

/// «А …», «и …», «теперь …» open a new request only before a command or a question.
const SOFT_STARTS: &[&str] = &["а", "и", "теперь", "тогда", "ещё", "еще", "так", "слушай", "слушайте", "смотри"];

/// Imperatives a request opens with.
const VERBS: &[&str] = &[
    "открой", "открывай", "закрой", "закрывай", "включи", "включай", "выключи", "выключай", "отключи", "запусти", "запускай",
    "сделай", "покажи", "показывай", "найди", "поищи", "ищи", "скажи", "расскажи", "объясни", "перескажи", "назови",
    "напиши", "прочитай", "прочти", "переведи", "посчитай", "проверь", "переключи", "поставь", "установи", "сверни",
    "разверни", "создай", "сохрани", "удали", "убери", "добавь", "верни", "отправь", "нажми", "перейди", "зайди",
    "выбери", "скопируй", "вставь", "помоги", "поменяй", "смени", "измени", "исправь", "переделай", "уменьши", "увеличи",
    "напомни", "запиши", "ответь", "продолжи", "остановись", "пауза",
];

/// Question words; a question needs a few more words to be the user's.
const QUESTIONS: &[&str] = &["что", "как", "где", "когда", "почему", "зачем", "сколько", "какой", "какая", "какое", "какие", "какую", "кто", "куда", "откуда"];

/// Casual words before the real opener («Ну, давай…», «Ой, нет…»).
const FILLER_LEADS: &[&str] = &["ну", "ой", "эй", "блин", "ладно", "окей", "ок", "слушай", "так"];

enum Steer {
    /// «Подожди.» — stop and listen.
    Hold,
    /// A correction or a new request.
    Words,
}

/// Lower-case words; a hyphen keeps «где-то» one word (not the question «где»).
fn plain_words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '-').trim_matches('-').to_lowercase())
        .filter(|w| !w.is_empty())
        .collect()
}

/// Built like an interruption of a spoken answer (any kind).
pub fn interrupts_reply(text: &str) -> bool {
    steering(text).is_some()
}

/// Is this phrase, said over JARVIS's answer, an interruption?
fn steering(text: &str) -> Option<Steer> {
    let all = plain_words(text);
    let lead = all.iter().take_while(|w| FILLER_LEADS.contains(&w.as_str())).count();
    let w = &all[lead..];
    let first = w.first()?.as_str();
    let rest = w.len() - 1;
    let verb_near = |from: usize| w.iter().skip(from).take(2).any(|x| VERBS.contains(&x.as_str()));
    let question_near = |from: usize| w.iter().skip(from).take(2).any(|x| QUESTIONS.contains(&x.as_str()));
    if HOLD_WORDS.contains(&first) {
        return Some(if rest == 0 { Steer::Hold } else { Steer::Words });
    }
    if DIRECTIVES.contains(&first) || (VERBS.contains(&first) && rest >= 1) {
        // «Продолжай» alone is no interruption: the paused answer just goes on.
        return Some(Steer::Words);
    }
    if STEER_STARTS.contains(&first) && rest >= 1 && (rest >= 2 || verb_near(1)) {
        return Some(Steer::Words);
    }
    if SOFT_STARTS.contains(&first) && (verb_near(1) || (question_near(1) && rest >= 2)) {
        return Some(Steer::Words);
    }
    if QUESTIONS.contains(&first) && rest >= 2 && verb_near(1) {
        return Some(Steer::Words);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idle() -> Context {
        Context::default()
    }
    /// The agent is working on a task, silently.
    fn working() -> Context {
        Context { busy: true, turn_active: true, turn_age: Duration::from_secs(40), ..Context::default() }
    }
    /// JARVIS is reading out its answer.
    fn speaking() -> Context {
        Context { busy: true, speaking: true, ..Context::default() }
    }
    fn amb(t: &str) -> Intent {
        Intent::Ambient { text: t.to_string(), cut: false }
    }
    fn cut(t: &str) -> Intent {
        Intent::Ambient { text: t.to_string(), cut: true }
    }
    fn req(t: &str) -> Intent {
        Intent::Request(t.to_string())
    }
    fn rep(t: &str) -> Intent {
        Intent::Replace(t.to_string())
    }

    #[test]
    fn repeated_and_named_stop_words_stop() {
        for t in ["Стоп.", "Stop.", "Стоп, стоп, стоп.", "Стоп-стоп-стоп!", "Джар, стоп!", "Джарвис, хватит!", "Эй, Джарвис, стоп, стоп", "Прекрати.", "Остановись!"] {
            assert!(is_stop(t), "must stop: {t}");
            assert_eq!(classify(t, &working()), Intent::Stop, "{t}");
        }
        assert!(!is_stop("Открой браузер"));
        assert!(!is_stop("Стоп, открой калькулятор"));
        assert!(!is_stop(""));
    }

    #[test]
    fn stop_followed_by_a_command_replaces_the_task() {
        // 11:18:58 — the whole phrase went to the agent as a new request
        // while «стоп» was ignored.
        let live = "Стоп, стоп, стоп. Алло, Джарвис. Открывай ведьму на в Яндекс.Браузере, на весь экран мне разверни.";
        assert_eq!(classify(live, &working()), rep("Открывай ведьму на в Яндекс.Браузере, на весь экран мне разверни."));
        assert_eq!(classify("Стоп, открой калькулятор", &working()), rep("открой калькулятор"));
        // A remark after the stop is only a stop.
        for t in ["Стоп, не то.", "Стоп, спасибо.", "Хватит, ладно."] {
            assert_eq!(classify(t, &working()), Intent::Stop, "{t}");
        }
    }

    #[test]
    fn the_name_interrupts_a_running_task() {
        assert_eq!(classify("Джарвис!", &working()), Intent::Call);
        assert_eq!(classify("Эй, Джарвис.", &working()), Intent::Call);
        assert_eq!(classify("Джарвис, нет, не то, открой ютуб.", &working()), rep("Джарвис, нет, не то, открой ютуб."));
        assert_eq!(classify("Джарвис, открой фильм Ведьма на киночке go.", &working()), rep("Джарвис, открой фильм Ведьма на киночке go."));
        // Misheard names heard live: «Жарвис», «Гарвис».
        assert_eq!(classify("Жарвис, сделай погромче.", &working()), rep("Жарвис, сделай погромче."));
        assert_eq!(classify("Гарвис, поставь на паузу.", &working()), rep("Гарвис, поставь на паузу."));
        // Over an answer being read out as well.
        assert_eq!(classify("Джарвис, подожди.", &speaking()), rep("Джарвис, подожди."));
        assert_eq!(classify("Джарвис", &speaking()), Intent::Call);
    }

    #[test]
    fn chatter_before_the_name_is_dropped() {
        // 11:18:04 — a command tangled up in a conversation about the film.
        let live = "Он сказал сначала... А Джарвис, открывай Ведьму. Будем смотреть Ведьму.";
        assert_eq!(classify(live, &working()), rep("Джарвис, открывай Ведьму. Будем смотреть Ведьму."));
        assert_eq!(classify(live, &idle()), req("Джарвис, открывай Ведьму. Будем смотреть Ведьму."));
    }

    #[test]
    fn room_talk_does_not_touch_a_running_task() {
        // All heard between 11:19 and 11:23 while the agent was clicking
        // through a site; each one used to restart or queue behind the task.
        for t in [
            "Не могу найти.",
            "Два месяца осталось.",
            "Короче, если что, поболтай с ним, скажи, чтобы он кинул.",
            "Минут пять и готово.",
            "Это не оно.",
            "Это не оно. Включи его, пожалуйста.",
            "Нет.",
            "Подожди, я сейчас.",
            "Сейчас, сейчас, сейчас, иду, сейчас.",
        ] {
            assert_eq!(classify(t, &working()), Intent::Ignore, "{t}");
        }
    }

    /// Everything the microphone took from the room on 2026-11-01 between
    /// 12:49 and 15:33 (film, a talk at home) as the recogniser wrote it —
    /// none of it may steer JARVIS while it works or speaks.
    const ROOM: &[&str] = &[
        "Возьмите срочно.", "Господи.", "Что вы", "Джой.", "Салют.", "Я не люблю суть.", "Ой.", "Это", "Таких ноги моих", "Да.",
        "Нужен.", "Забыт.", "Я", "Сколько стоит?", "Может быть чёрное?", "Ну давай.", "Эй, знаешь,", "Хорошо, да?", "Если вы",
        "Чего?", "Сейчас.", "Скринс.", "Спазмный.", "Побежишь.", "Четыре.", "Ну, как тебя с ним сейчас так маленький.", "Ско.",
        "Что вы?", "Через два.", "Постоянно.", "Рядом с короткой выключаем.", "Ты закрываешь дверь и всё?", "Я не могу с ней говорить. Можешь?",
        "Шестнадцатый брат.", "Мы что-то там купили.", "Тридцать максимум тут.", "Где-то на час.", "Там обычно всё быстро проходит.",
        "Соленья, варенье.", "Бабушку ты можешь.", "Можешь?", "Десять.", "Как ты должен быть? Нет.", "Через 65 приблизительно.", "А там.",
        "Нет.", "Естественно.", "Если тебе положи.", "Помню, как на этого парня.", "Ладно.", "Тогда ну хотя бы попробуй.",
        "Ничего не забыл. Ничего не забыл.", "Ну, иди поешь. Сейчас, сейчас, сейчас, иду, сейчас.", "Садюсь.", "Около последней ставить.",
        "Вот такое шино.", "Ну что?", "Чё, не надо?", "Штор.", "Трусы одели.", "С почками от нашего", "Какая погода?", "Сбер.", "Жи.",
        "Давай сразу этот.", "Подтверждение.", "Сбер, продолжать.", "Па.", "Востока.", "Посмотрите на пружинку.", "Отчин.", "Где?",
        "Включи ласты.", "Алиса,", "Уличны.", "Продолжай.", "Спина моя.", "А он.", "Зажимай.", "Что?", "Грязь.", "Семья стирала чеки.",
        "И мы сегодня знаем об этом только потому, что для детей", "Я начинаю", "Великолепно.", "Что на тебя?",
    ];

    /// Room phrases built exactly like a request. Over a spoken answer they
    /// now interrupt it — the price of being able to cut JARVIS off without
    /// its name (asked for 2026-11-18). Silent work stays untouchable.
    const ROOM_LIKE_A_REQUEST: &[&str] = &["Давай сразу этот.", "Включи ласты."];

    #[test]
    fn a_real_afternoon_of_room_talk_never_steers_a_busy_jarvis() {
        for t in ROOM {
            assert_eq!(classify(t, &working()), Intent::Ignore, "working: {t}");
            if !ROOM_LIKE_A_REQUEST.contains(t) {
                assert_eq!(classify(t, &speaking()), Intent::Ignore, "speaking: {t}");
            }
        }
    }

    #[test]
    fn the_user_can_cut_into_a_spoken_answer_without_the_name() {
        for t in [
            "Нет, давай короче.",
            "Нет, не то, открой проводник.",
            "Подожди, открой лучше Загрузки.",
            "Погоди, не в эту папку.",
            "Короче.",
            "Скажи проще.",
            "Ну давай лучше погоду на завтра.",
            "А какая погода завтра?",
            "А открой ютуб.",
            "Лучше включи музыку.",
            "Повтори последнее.",
            "Открой браузер.",
        ] {
            assert_eq!(classify(t, &speaking()), cut(t), "{t}");
        }
        // «Подожди» alone: stop talking, listen.
        assert_eq!(classify("Подожди.", &speaking()), Intent::Call);
        assert_eq!(classify("Секунду!", &speaking()), Intent::Call);
        // «Продолжай»: no interruption, the paused answer goes on.
        assert_eq!(classify("Продолжай.", &speaking()), Intent::Ignore);
        // The same words over silent work are still the room.
        for t in ["Нет, давай короче.", "Открой браузер.", "А какая погода завтра?", "Подожди."] {
            assert_eq!(classify(t, &working()), Intent::Ignore, "working: {t}");
        }
    }

    #[test]
    fn room_talk_does_not_interrupt_a_reply_through_speakers() {
        for t in ["Сегодня хорошая погода.", "Ты закрываешь дверь и всё?", "Фильм на вечер."] {
            assert_eq!(classify(t, &speaking()), Intent::Ignore, "{t}");
        }
    }

    #[test]
    fn the_rest_of_a_thought_joins_only_a_fresh_turn() {
        let fresh = Context { continues: true, turn_age: Duration::from_secs(2), ..working() };
        assert_eq!(classify("и найди погоду", &fresh), req("и найди погоду"));
        // Minutes into a task a phrase that follows closely is not a thought.
        let late = Context { continues: true, turn_age: Duration::from_secs(40), ..working() };
        assert_eq!(classify("Два месяца осталось.", &late), Intent::Ignore);
        // Not a continuation, even when fresh.
        let other = Context { turn_age: Duration::from_secs(2), ..working() };
        assert_eq!(classify("и найди погоду", &other), Intent::Ignore);
        // The answer is being read out and they go on with the thought.
        let reading = Context { continues: true, ..speaking() };
        assert_eq!(classify("и яркость", &reading), req("и яркость"));
    }

    #[test]
    fn in_headphones_plain_words_still_interrupt_a_spoken_answer() {
        let phones = Context { headphones: true, ..speaking() };
        assert_eq!(classify("Сегодня хорошая погода.", &phones), cut("Сегодня хорошая погода."));
        // But not a running task: nothing is being read out, the words are the room.
        let phones_working = Context { headphones: true, ..working() };
        assert_eq!(classify("Не могу найти.", &phones_working), Intent::Ignore);
    }

    #[test]
    fn when_idle_follow_ups_built_like_requests_need_no_name_but_the_model_judges_them() {
        assert_eq!(classify("Какое у нас настроение?", &idle()), amb("Какое у нас настроение?"));
        assert_eq!(classify("А завтра?", &idle()), amb("А завтра?"));
        assert_eq!(classify("Открой загрузки.", &idle()), amb("Открой загрузки."));
        assert_eq!(classify("А ты что думаешь?", &idle()), amb("А ты что думаешь?"));
        // Exact device commands are certain: no model needed.
        assert_eq!(classify("Сделай погромче.", &idle()), req("Сделай погромче."));
        assert_eq!(classify("Пауза.", &idle()), req("Пауза."));
        assert_eq!(classify("Джарвис, сделай погромче.", &idle()), req("Джарвис, сделай погромче."));
        assert_eq!(classify("Джарвис!", &idle()), Intent::Call);
        assert_eq!(classify("Стоп.", &idle()), Intent::Stop);
        assert_eq!(classify("Стоп, открой калькулятор", &idle()), req("открой калькулятор"));
    }

    /// Answered live on 2026-11-20/21 in the minute after a reply, while a
    /// song, a game or a talk at home went on. Not one may get an answer.
    const HEARD_AFTER_A_REPLY: &[&str] = &[
        "Наверное, нигде. Вряд ли он с России приедет", "Это моя", "Это надо сюда положить. Короче, моя руки.",
        "Никак пойдём найдём на эльцу.", "Вачем замуж.", "В процессе загрузки.", "Да. О. Вот, тут уже поинтереснее.",
        "Дыши, брат.", "Я не знаю, как бы тебе это показать.", "Можешь удалить? Случайно удалить.",
    ];

    #[test]
    fn statements_heard_after_a_reply_are_the_rooms() {
        for t in HEARD_AFTER_A_REPLY {
            assert_eq!(classify(t, &idle()), Intent::Ignore, "{t}");
        }
        // The afternoon of room talk, too — the few built exactly like a
        // request or a question to JARVIS go to the model, which may stay silent.
        const MODEL_JUDGES: &[&str] = &["Включи ласты.", "Ты закрываешь дверь и всё?", "Что на тебя?"];
        for t in ROOM {
            let want = if MODEL_JUDGES.contains(t) { amb(t) } else { Intent::Ignore };
            assert_eq!(classify(t, &idle()), want, "idle: {t}");
        }
    }

    #[test]
    fn while_a_song_or_a_film_plays_only_player_commands_count_unnamed() {
        crate::platform::media::set_cached(crate::platform::media::MediaState { available: true, status: "playing".into(), ..Default::default() });
        let music = Context { media_playing: true, ..idle() };
        for t in ["Включи ласты.", "Открой мне дверь.", "А ты что думаешь?", "Музыка!", "Громче!"] {
            assert_eq!(classify(t, &music), Intent::Ignore, "{t}");
        }
        // Player commands count from anyone (the user's choice, 2026-11-21).
        for t in ["Дальше.", "Назад!", "Пауза"] {
            assert_eq!(classify(t, &music), req(t), "{t}");
        }
        // …but not over JARVIS's own voice through the speakers, nor into his work.
        assert_eq!(classify("Пауза.", &Context { media_playing: true, ..speaking() }), Intent::Ignore);
        assert_eq!(classify("Дальше.", &Context { media_playing: true, ..working() }), Intent::Ignore);
        // With headphones there is no echo: a player word over the reply counts.
        let in_headphones = Context { media_playing: true, headphones: true, ..speaking() };
        assert_eq!(classify("Пауза.", &in_headphones), rep("Пауза."));
        // The name, or «стоп» opening the phrase, still does.
        assert_eq!(classify("Джарвис, дальше.", &music), req("Джарвис, дальше."));
        assert_eq!(classify("Стоп музыка", &music), req("Стоп музыка"));
        // Right after the user's own skip, «дальше… дальше…» goes on.
        let chain = Context { media_playing: true, media_chain: true, ..idle() };
        assert_eq!(classify("Дальше.", &chain), req("Дальше."));
        assert_eq!(classify("Пауза.", &chain), req("Пауза."));
        // …even over the track announcement, which it cuts off.
        let announcing = Context { media_playing: true, media_chain: true, ..speaking() };
        assert_eq!(classify("Дальше.", &announcing), rep("Дальше."));
        // Over a reply with music on, only «подожди» holds JARVIS.
        let reply_over_music = Context { media_playing: true, ..speaking() };
        assert_eq!(classify("Открой браузер.", &reply_over_music), Intent::Ignore);
        assert_eq!(classify("Подожди.", &reply_over_music), Intent::Call);
    }

    #[test]
    fn player_commands_from_anyone_are_only_player_words() {
        // Heard live 2026-11-21: «Играй.», «Верни музыку.», «Пауза.» with the
        // conversation closed — all lost; the user wants them to work.
        crate::platform::media::set_cached(crate::platform::media::MediaState { available: true, status: "playing".into(), ..Default::default() });
        for t in ["Играй.", "Верни музыку.", "Пауза.", "Дальше.", "Назад.", "Следующий трек.", "Стоп.", "Хватит!"] {
            assert_eq!(player_command(t).as_deref(), Some(t), "goes on: {t}");
        }
        // Several in one breath (heard live): the last one is what to do.
        assert_eq!(player_command("Пауза. Играй. Верни музыку.").as_deref(), Some("Верни музыку."));
        assert_eq!(player_command("Дальше. Дальше.").as_deref(), Some("Дальше."));
        assert_eq!(player_command("Играй. Пауза.").as_deref(), Some("Пауза."));
        assert_eq!(player_command("Стоп, стоп, стоп.").as_deref(), Some("стоп."));
        assert_eq!(classify("Дальше. Дальше.", &Context { media_playing: true, ..idle() }), req("Дальше."));
        assert_eq!(classify("Пауза. Играй. Верни музыку.", &idle()), req("Верни музыку."));
        assert_eq!(classify("Стоп, стоп, стоп.", &speaking()), Intent::Stop);
        for t in ["Пауза. Я сейчас приду.", "Дальше. А что потом?", "Играй, Вася, играй, а мы послушаем."] {
            assert_eq!(player_command(t), None, "not only player words: {t}");
        }
        for t in ["Играй в футбол.", "Дальше?", "Включи свет.", "Ну давай дальше рассказывай.", "Стоп, а где ключи?", "Открой браузер.", "Привет.", "Музыка!", "Тихо!", "Отмена."] {
            assert_eq!(player_command(t), None, "not a player command: {t}");
        }
    }

    #[test]
    fn talk_to_another_assistant_is_never_for_jarvis() {
        for t in ["Алиса, нежно-белый цвет.", "Окей, Гугл, какая погода?", "Эй, Сири, позвони маме", "Маруся, включи музыку"] {
            assert_eq!(classify(t, &idle()), Intent::Ignore, "{t}");
            assert_eq!(classify(t, &working()), Intent::Ignore, "{t}");
        }
        assert_eq!(classify("Джарвис, спроси у Алисы погоду", &idle()), req("Джарвис, спроси у Алисы погоду"));
    }

    #[test]
    fn a_stop_at_the_end_of_a_phrase_stops() {
        let live = "Давай я тебе скину. Ничего, Жарвис, всё, Джарвис, хватит. Стоп.";
        assert_eq!(classify(live, &idle()), Intent::Stop);
        assert_eq!(classify(live, &speaking()), Intent::Stop);
        assert_eq!(classify("Ну всё. Хватит.", &working()), Intent::Stop);
        assert_eq!(classify("Слушай, Джарвис, хватит", &idle()), Intent::Stop);
        // A story with «стоп» in it is not a stop.
        assert_eq!(classify("Он сказал: стоп.", &working()), Intent::Ignore);
        assert_eq!(classify("Мама, стой!", &working()), Intent::Ignore);
    }

    #[test]
    fn closing_words_end_the_conversation() {
        // Named: a short goodbye.
        assert_eq!(classify("Спасибо, Джарвис.", &idle()), Intent::Dismiss("Обращайтесь."));
        assert_eq!(classify("Всё, спасибо, Жар, если можешь выключаться.", &idle()), Intent::Dismiss("Обращайтесь."));
        assert_eq!(classify("Джарвис, ты молодец", &idle()), Intent::Dismiss("Рад стараться."));
        assert_eq!(classify("Иди нахрен, Жарвюс.", &speaking()), Intent::Dismiss("Понял."));
        assert_eq!(classify("Джарвис, отбой", &working()), Intent::Dismiss("Хорошо."));
        // Unnamed: silently — the room's «ладно, я пошёл» gets no answer.
        for t in ["Ладно, я пошёл.", "Всё, спасибо.", "Ничего не надо.", "Отбой."] {
            assert_eq!(classify(t, &idle()), Intent::Dismiss(""), "{t}");
        }
        // A thank-you that goes on with a request is a request.
        assert_eq!(classify("Спасибо, Джарвис, а теперь открой почту", &idle()), req("Джарвис, а теперь открой почту"));
        assert_eq!(classify("Всё хорошо?", &idle()), Intent::Ignore);
    }

    #[test]
    fn an_answer_to_jarvis_question_needs_no_name() {
        let asked = Context { asked: true, ..idle() };
        assert_eq!(classify("В Загрузки.", &asked), amb("В Загрузки."));
        assert_eq!(classify("Да, давай.", &asked), amb("Да, давай."));
        // A monologue is not an answer.
        let long = "Вчера мы ездили к бабушке и там было очень много снега везде";
        assert_eq!(classify(long, &asked), Intent::Ignore);
    }
}
