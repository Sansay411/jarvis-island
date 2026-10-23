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
    // Heard live: «Спасибо, Жервис», «Дарвис, стоп».
    "жервис", "дарвис", "джервиз", "жарвиз", "дарвиз", "джарвас", "жервиз",
];

/// Words that may come before the name: «эй, Джарвис», «окей, Джар».
const LEADS: &[&str] = &[
    "эй", "окей", "ок", "хей", "слушай", "привет", "здравствуй", "здравствуйте", "алло", "ну", "а", "так", "о", "йо", "хай",
];

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
const SHORT: &[&str] = &["жар", "дар", "чар", "джал", "жал", "джа", "дарв", "жарв"];

/// Words that sound like the name but are not it.
const NOT_NAMES: &[&str] = &["сервис", "сервиз", "дарвин", "харви", "марвин", "чарли", "жарить", "жаркий"];

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
    let c = w.replace("дж", "ж").replace('ч', "ж").replace('з', "с");
    levenshtein(&c, "жарвис") <= 2
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
        for t in ["Дар.", "Эрвис.", "Джал, открой браузер", "Жар!", "Джервиз, сколько времени?", "Дарвис, включи музыку", "Жарвиса", "Чарвис, пауза", "Эй, джа рвис", "Джарви, громче", "Джарвисом", "Ярвис, привет"] {
            assert!(yes(t), "must wake: {t}");
        }
    }

    #[test]
    fn similar_ordinary_words_do_not() {
        for t in ["Сервис не работает", "Жарко сегодня", "Дар речи пропал", "Чарли, иди сюда", "Жарить картошку"] {
            assert!(!yes(t), "must not wake: {t}");
        }
    }

    #[test]
    fn long_monologues_do_not() {
        let long = format!("Джарвис {}", "и ещё одно слово ".repeat(10));
        assert!(!yes(&long));
    }
}
