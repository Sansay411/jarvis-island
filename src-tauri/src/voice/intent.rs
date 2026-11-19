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
    /// Cancel the running turn or reply and handle these words instead.
    Replace(String),
    /// An ordinary request (or the rest of one); the words to use.
    Request(String),
    /// Not for JARVIS.
    Ignore,
}

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

/// Decide what a recognised phrase means. `ctx.busy` is the heart of it:
/// while JARVIS works or speaks, the room must not steer it.
pub fn classify(text: &str, ctx: &Context) -> Intent {
    let take = |words: &str| {
        let words = words.to_string();
        if ctx.busy { Intent::Replace(words) } else { Intent::Request(words) }
    };
    match stop_prefix(text) {
        Some(StopRest::Alone | StopRest::Remark) => return Intent::Stop,
        Some(StopRest::Command(rest)) => return take(rest),
        None => {}
    }
    match wake::addressed(text) {
        Some(a) if a.only_name => return Intent::Call,
        Some(_) => return take(text),
        None => {}
    }
    // The name after a few words of chatter: the command starts there.
    let within = if ctx.busy { NAME_WITHIN_BUSY } else { NAME_WITHIN_IDLE };
    if let Some(command) = wake::from_name(text, within) {
        return take(command);
    }
    if !ctx.busy {
        return Intent::Request(text.to_string());
    }
    // Unnamed words over a task or a reply: the user only if they go on with
    // the thought they had just begun, or — nothing else being said aloud
    // and no echo — talk over an answer in headphones.
    let thought = ctx.continues && (!ctx.turn_active || ctx.turn_age < MERGE_WINDOW);
    if thought || (ctx.headphones && !ctx.turn_active) {
        return Intent::Request(text.to_string());
    }
    // Talking over a spoken answer: an interruption has a recognisable
    // shape. Anything else is the room, and the paused reply goes on.
    if ctx.speaking {
        return match steering(text) {
            Some(Steer::Hold) => Intent::Call,
            Some(Steer::Words) => Intent::Replace(text.to_string()),
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
        Context { busy: false, speaking: false, turn_active: false, continues: false, turn_age: Duration::ZERO, headphones: false }
    }
    /// The agent is working on a task, silently.
    fn working() -> Context {
        Context { busy: true, speaking: false, turn_active: true, continues: false, turn_age: Duration::from_secs(40), headphones: false }
    }
    /// JARVIS is reading out its answer.
    fn speaking() -> Context {
        Context { busy: true, speaking: true, turn_active: false, continues: false, turn_age: Duration::ZERO, headphones: false }
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
            assert_eq!(classify(t, &speaking()), rep(t), "{t}");
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
        assert_eq!(classify("Сегодня хорошая погода.", &phones), req("Сегодня хорошая погода."));
        // But not a running task: nothing is being read out, the words are the room.
        let phones_working = Context { headphones: true, ..working() };
        assert_eq!(classify("Не могу найти.", &phones_working), Intent::Ignore);
    }

    #[test]
    fn when_idle_follow_ups_need_no_name() {
        assert_eq!(classify("Какое у нас настроение?", &idle()), req("Какое у нас настроение?"));
        assert_eq!(classify("Джарвис, сделай погромче.", &idle()), req("Джарвис, сделай погромче."));
        assert_eq!(classify("Джарвис!", &idle()), Intent::Call);
        assert_eq!(classify("Стоп.", &idle()), Intent::Stop);
        assert_eq!(classify("Стоп, открой калькулятор", &idle()), req("открой калькулятор"));
    }
}
