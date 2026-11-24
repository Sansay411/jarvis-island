//! «Джарвис, …» — deciding whether a recognised phrase is addressed to
//! JARVIS while it waits for its name.
//!
//! Recognition runs on every phrase (GigaAM, ~0.1 s), so the decision is made
//! on text and stays strict: the name must open the phrase (optionally after
//! «эй», «окей», «привет»…) or close a short one («спасибо, Джарвис»).
//! The name in the middle of a conversation, similar words («жар»,
//! «Дарвин», «Харви») and long monologues (TV, videos) do not count.

/// Spellings of the name the recogniser produces, lower-case, «ё» as «е».
/// GigaAM test (7 voices): «Джарвис» 6×, once in Latin «Javis».
const NAMES: &[&str] = &[
    "джарвис", "джарвиз", "джарвиса", "джарвису", "джарвисом", "джарвисе", "джарви", "джарвес", "джервис", "джарбис",
    "джарес", "джарис", "джар", "джарр", "жарвис", "чарвис", "джавис", "джарвиш", "jarvis", "javis", "jarvi", "jarviz",
    // Heard live: «Спасибо, Жервис», «Дарвис, стоп», «Джанс.» (2026-11-21).
    "жервис", "дарвис", "джервиз", "жарвиз", "дарвиз", "джарвас", "жервиз", "джанс",
    // Heard live: «Jarves. Пауза.», «…что ещё в Жарвесе?» (2026-11-21/23).
    "jarves", "жарвес", "жарвеса", "жарвесе",
];

/// Words that may come before the name: «эй, Джарвис», «окей, Джар».
pub(super) const LEADS: &[&str] = &[
    "эй", "окей", "ок", "хей", "слушай", "привет", "здравствуй", "здравствуйте", "алло", "ну", "а", "так", "о", "йо", "хай",
];

/// Hello words: a call with one of them is greeted back, not answered «Слушаю».
const GREETINGS: &[&str] = &["привет", "приветик", "здравствуй", "здравствуйте", "хай", "салют", "здорово", "hello", "hi"];

/// The phrase says hello.
pub fn greets(text: &str) -> bool {
    words(text).iter().any(|w| GREETINGS.contains(&w.as_str()))
}

/// Phrases longer than this are a monologue, not a call.
const MAX_WORDS: usize = 24;
/// A name at the end counts only in short phrases.
const MAX_WORDS_NAME_LAST: usize = 6;

#[derive(Debug, PartialEq, Eq)]
pub struct Addressed {
    /// Only the name (and a greeting) was said — JARVIS should just answer.
    pub only_name: bool,
}

fn words(text: &str) -> Vec<String> {
    let raw: Vec<String> = text
        .to_lowercase()
        .replace('ё', "е")
        .split(|c: char| !c.is_alphabetic())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect();
    // The recogniser sometimes splits the name: «Джар Вис», «Джа Рвис».
    let mut out: Vec<String> = Vec::with_capacity(raw.len());
    for w in raw {
        if let Some(prev) = out.last_mut() {
            let joined = format!("{prev}{w}");
            if prev.chars().count() <= 4 && (matches!(joined.as_str(), "джарвис" | "джарвиз" | "джарвиса") || sounds_like_name(&joined)) {
                *prev = joined;
                continue;
            }
        }
        out.push(w);
    }
    out
}

/// What is left of the name when the recogniser cuts it («Дар.», «Джал»).
/// These are ordinary words too («жар»), so they count only when nothing
/// follows them but punctuation — see `addressed`.
pub(super) const SHORT: &[&str] = &["жар", "дар", "чар", "джал", "жал", "джа", "дарв", "жарв"];

/// Words that sound like the name but are not it.
const NOT_NAMES: &[&str] = &["сервис", "сервиз", "дарвин", "харви", "марвин", "чарли", "жарить", "жаркий", "нарвись", "нарвис"];

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            cur[j + 1] = (prev[j] + usize::from(ca != cb)).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[b.len()]
}

/// Sounds like «Джарвис»: within two letters of it once «дж», «ч» and «ж»
/// are one sound and «з» is «с» — «Эрвис», «Джервиз», «Жарвиса», «Дарвис».
fn sounds_like_name(w: &str) -> bool {
    let n = w.chars().count();
    if !(4..=10).contains(&n) || NOT_NAMES.iter().any(|x| w.starts_with(x)) {
        return false;
    }
    // A soft sign the recogniser adds («Ярвись») is no sound of its own.
    let c = w.replace("дж", "ж").replace('ч', "ж").replace('з', "с").replace(['ь', 'ъ'], "");
    // Two letters off is loose: the sound must still hold «рв» or begin like
    // the name — «Ларис!» (to a Лариса) and «Парис» are not JARVIS.
    (c.starts_with('ж') || c.contains("рв")) && levenshtein(&c, "жарвис") <= 2
}

/// Close to the name, for a decode that mangled it after the early cue had
/// already heard it («Джас», «Жаз»): within three letters.
pub(super) fn near_name(w: &str) -> bool {
    if !(3..=9).contains(&w.chars().count()) {
        return false;
    }
    let c = w.replace("дж", "ж").replace('ч', "ж").replace('з', "с");
    is_name(w) || levenshtein(&c, "жарвис") <= 3
}

/// The name, or a greeting glued to it («Эйджарвис» — 2 of 7 voices).
pub fn is_name(w: &str) -> bool {
    let full = |w: &str| NAMES.contains(&w) || sounds_like_name(w);
    full(w) || LEADS.iter().any(|l| w.strip_prefix(l).is_some_and(full))
}

/// A cut-off name followed only by punctuation or nothing: «Дар.», «Джал, …».
fn short_name_alone(text: &str, w: &str) -> bool {
    if !SHORT.contains(&w) {
        return false;
    }
    let lower = text.to_lowercase().replace('ё', "е");
    let Some(at) = lower.find(w) else { return false };
    let after = lower[at + w.len()..].chars().next();
    after.is_none_or(|c| !c.is_alphabetic() && !c.is_whitespace())
}

/// The words of `text` (lower-case, «ё» as «е») with the byte offset where
/// each begins, so a slice of the original keeps its punctuation.
pub(super) fn spans(text: &str) -> Vec<(usize, String)> {
    let mut out: Vec<(usize, String)> = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in text.char_indices().chain(std::iter::once((text.len(), ' '))) {
        match (c.is_alphabetic(), start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                out.push((s, text[s..i].to_lowercase().replace('ё', "е")));
                start = None;
            }
            _ => {}
        }
    }
    out
}

/// The rest of a remark from the name on: «Он сказал… А Джарвис, открывай
/// Ведьму» → «Джарвис, открывай Ведьму». The name must be among the first
/// `within + 1` words and something besides greetings must follow it.
/// Cut-off forms («Дар», «Жар») are ordinary words here and do not count.
pub fn from_name(text: &str, within: usize) -> Option<&str> {
    let w = spans(text);
    let at = w.iter().take(within + 1).position(|(_, word)| is_name(word))?;
    let follows = w[at + 1..].iter().any(|(_, word)| !is_name(word) && !LEADS.contains(&word.as_str()));
    follows.then(|| &text[w[at].0..])
}

pub fn addressed(text: &str) -> Option<Addressed> {
    let w = words(text);
    if w.is_empty() || w.len() > MAX_WORDS {
        return None;
    }
    let named = |word: &str| is_name(word) || short_name_alone(text, word);
    let first = w.iter().take(4).position(|word| !LEADS.contains(&word.as_str()))
        .filter(|at| named(&w[*at]));
    let last = (w.len() <= MAX_WORDS_NAME_LAST && named(&w[w.len() - 1])).then(|| w.len() - 1);
    let at = first.or(last)?;
    let rest = w.iter().enumerate().filter(|(i, word)| *i != at && !named(word) && !LEADS.contains(&word.as_str())).count();
    Some(Addressed { only_name: rest == 0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn yes(t: &str) -> bool {
        addressed(t).is_some()
    }

    #[test]
    fn hello_is_a_greeting_not_a_bare_call() {
        assert!(greets("Привет, Джарвис."));
        assert!(greets("Джарвис, здравствуй!"));
        assert!(!greets("Эй, Джарвис"));
        assert!(!greets("Джарвис, открой браузер."));
    }

    #[test]
    fn name_at_the_start_wakes() {
        assert_eq!(addressed("Джарвис!"), Some(Addressed { only_name: true }));
        assert_eq!(addressed("Эй, Джарвис"), Some(Addressed { only_name: true }));
        assert_eq!(addressed("Джарвис, открой браузер."), Some(Addressed { only_name: false }));
        assert_eq!(addressed("Джар, сделай потише"), Some(Addressed { only_name: false }));
        assert!(yes("Окей, Джарвис, какая погода?"));
        assert!(yes("Привет, Джарвис."));
        assert!(yes("Джарвиз, ты тут?"));
        assert_eq!(addressed("Эйджарвис."), Some(Addressed { only_name: true }));
        assert_eq!(addressed("Javis"), Some(Addressed { only_name: true }));
        assert_eq!(addressed("Джар Вис, открой почту."), Some(Addressed { only_name: false }));
        assert_eq!(addressed("Джа рвис!"), Some(Addressed { only_name: true }));
    }

    #[test]
    fn short_lead_chains_and_recognition_variants_wake_but_mentions_do_not() {
        assert_eq!(addressed("А ну, Джарвис!"), Some(Addressed { only_name:true }));
        assert!(yes("Эй, слушай, Джарес, какая громкость?"));
        assert!(yes("Джарис, ты здесь?"));
        assert!(!yes("Я рассказал, что Джарес помогает мне работать"));
        assert!(!yes("А ну, Дарвин, привет"));
    }

    #[test]
    fn name_at_the_end_of_a_short_phrase_wakes() {
        assert_eq!(addressed("Спасибо, Джарвис."), Some(Addressed { only_name: false }));
        assert!(yes("Открой браузер, Джарвис"));
        assert!(!yes("Вчера я долго смотрел интервью, где говорили про Джарвис"));
    }

    #[test]
    fn similar_words_and_mentions_do_not() {
        assert!(!yes("Жар у меня уже спал."));
        assert!(!yes("Дарвин написал книгу о происхождении видов."));
        assert!(!yes("Харви, привет."));
        assert!(!yes("Я смотрел фильм, где Джарвис помогает Тони Старку."));
        assert!(!yes("Джаз сегодня вечером в клубе"));
        assert!(!yes(""));
        assert!(!yes("..."));
    }

    #[test]
    fn cut_and_misheard_names_wake() {
        for t in ["Дар.", "Эрвис.", "Джал, открой браузер", "Жар!", "Джервиз, сколько времени?", "Дарвис, включи музыку", "Жарвиса", "Чарвис, пауза", "Эй, джа рвис", "Джарви, громче", "Джарвисом", "Ярвис, привет", "Ярвись дальше.", "Иди нахрен, Жарвюс.", "А-а, Джарли, что ты умеешь?", "Джанс.", "Джанс, пауза"] {
            assert!(yes(t), "must wake: {t}");
        }
    }

    #[test]
    fn similar_ordinary_words_do_not() {
        for t in ["Сервис не работает", "Жарко сегодня", "Дар речи пропал", "Чарли, иди сюда", "Жарить картошку", "Ларис, иди сюда", "Парис!", "Нарвись!", "Включи джаз", "Это джаз.", "Есть шанс."] {
            assert!(!yes(t), "must not wake: {t}");
        }
    }

    #[test]
    fn command_is_taken_from_the_name_on_when_chatter_comes_first() {
        // Heard live 2026-11-01 while watching a film with company.
        let live = "Он сказал сначала... А Джарвис, открывай Ведьму. Будем смотреть Ведьму.";
        assert_eq!(from_name(live, 5), Some("Джарвис, открывай Ведьму. Будем смотреть Ведьму."));
        assert_eq!(from_name("Алло, Джарвис. Открывай ведьму", 4), Some("Джарвис. Открывай ведьму"));
        // The name alone, or only greetings after it, is not a command.
        assert_eq!(from_name("Ну вот, Джарвис", 4), None);
        assert_eq!(from_name("Ну вот, Джарвис, эй", 4), None);
        // A name too deep in the remark is a mention, not a call.
        assert_eq!(from_name("Мы вчера смотрели фильм где Джарвис помогает Тони", 4), None);
        // Ordinary words that merely sound like a cut name.
        assert_eq!(from_name("Жар у меня уже спал", 4), None);
        assert_eq!(from_name("", 4), None);
    }

    #[test]
    fn word_spans_point_into_the_original_text() {
        let t = "Эй, Джарвис! Ёлка";
        let w = spans(t);
        assert_eq!(w.iter().map(|(_, x)| x.as_str()).collect::<Vec<_>>(), ["эй", "джарвис", "елка"]);
        assert_eq!(&t[w[1].0..w[1].0 + "Джарвис".len()], "Джарвис");
        assert_eq!(&t[w[2].0..], "Ёлка");
    }

    #[test]
    fn long_monologues_do_not() {
        let long = format!("Джарвис {}", "и ещё одно слово ".repeat(10));
        assert!(!yes(&long));
    }
}
