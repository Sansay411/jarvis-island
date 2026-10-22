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
    "джар", "джарр", "jarvis", "javis", "jarvi",
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
    text.to_lowercase()
        .replace('ё', "е")
        .split(|c: char| !c.is_alphabetic())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// The name, or a greeting glued to it («Эйджарвис» — 2 of 7 voices).
pub(super) fn is_name(w: &str) -> bool {
    NAMES.contains(&w) || LEADS.iter().any(|l| w.strip_prefix(l).is_some_and(|rest| NAMES.contains(&rest)))
}

pub fn addressed(text: &str) -> Option<Addressed> {
    let w = words(text);
    if w.is_empty() || w.len() > MAX_WORDS {
        return None;
    }
    let first = if is_name(&w[0]) {
        Some(0)
    } else if w.len() > 1 && LEADS.contains(&w[0].as_str()) && is_name(&w[1]) {
        Some(1)
    } else {
        None
    };
    let last = (w.len() <= MAX_WORDS_NAME_LAST && is_name(&w[w.len() - 1])).then(|| w.len() - 1);
    let at = first.or(last)?;
    let rest = w.iter().enumerate().filter(|(i, word)| *i != at && !is_name(word) && !LEADS.contains(&word.as_str())).count();
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
    fn long_monologues_do_not() {
        let long = format!("Джарвис {}", "и ещё одно слово ".repeat(10));
        assert!(!yes(&long));
    }
}
