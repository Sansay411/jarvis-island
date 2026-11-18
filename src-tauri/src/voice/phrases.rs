//! Turning the agent's streamed text into speakable phrases: split at
//! sentence ends as deltas arrive (so speech starts before the answer is
//! complete) and strip Markdown, code and URLs that make no sense aloud.

/// Accumulates streamed text and yields complete sentences.
#[derive(Default)]
pub struct PhraseSplitter {
    buf: String,
    /// The first speakable phrase was cut: later ones wait for full sentence ends.
    started: bool,
}

/// Turbo first-clause flush: on the very first emitted chunk of a response,
/// flush on `,`, `—`, `–`, `:`, `;` (or spaced `-`) once >= 24 chars (3–5 words)
/// have arrived so synthesis begins ~2–3× sooner.
const MIN_FIRST_CLAUSE: usize = 24;

/// Short fragments are held back and joined with the next sentence so
/// speech doesn't stutter phrase by phrase.
const MIN_PHRASE: usize = 24;

impl PhraseSplitter {
    pub fn push(&mut self, delta: &str) -> Vec<String> {
        self.buf.push_str(delta);
        let mut out = Vec::new();
        loop {
            let chars: Vec<(usize, char)> = self.buf.char_indices().collect();
            let mut cut = None;
            for (i, &(pos, c)) in chars.iter().enumerate() {
                let prev = i.checked_sub(1).and_then(|p| chars.get(p)).map(|&(_, pc)| pc);
                let next = chars.get(i + 1).map(|&(_, n)| n);
                let spaced = next.is_some_and(char::is_whitespace);
                let prev_spaced = prev.is_some_and(char::is_whitespace);
                let len = || i + 1;
                let boundary = match c {
                    '\n' => true,
                    '.' | '!' | '?' | '…' => spaced && len() >= MIN_PHRASE,
                    ',' | ':' | '—' | '–' => spaced && !self.started && len() >= MIN_FIRST_CLAUSE,
                    ';' => {
                        if !self.started {
                            spaced && len() >= MIN_FIRST_CLAUSE
                        } else {
                            false
                        }
                    }
                    '-' => prev_spaced && spaced && !self.started && len() >= MIN_FIRST_CLAUSE,
                    _ => false,
                };
                if boundary {
                    cut = Some(pos + c.len_utf8());
                    break;
                }
            }
            let Some(at) = cut else { break };
            let phrase: String = self.buf.drain(..at).collect();
            let s = speakable(&phrase);
            if !s.is_empty() {
                self.started = true;
                out.push(s);
            }
        }
        out
    }

    /// The rest of the text once the answer is complete.
    pub fn finish(&mut self) -> Option<String> {
        let s = speakable(&std::mem::take(&mut self.buf));
        (!s.is_empty()).then_some(s)
    }
}

/// Split a complete text into speakable phrases using `PhraseSplitter` rules
/// (fast first clause, then natural sentences) so standalone `tts_speak` starts
/// streaming the first clause immediately while queueing the rest.
pub fn split_all(text: &str) -> Vec<String> {
    let mut sp = PhraseSplitter::default();
    let mut out = sp.push(text);
    if let Some(tail) = sp.finish() {
        out.push(tail);
    }
    out
}

/// Markdown and other written-only notation → plain speakable text.
pub fn speakable(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_fence = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        // Headings, list markers, quotes, table pipes.
        let t = t.trim_start_matches('#').trim_start_matches('>').trim_start();
        let t = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")).or_else(|| t.strip_prefix("• ")).unwrap_or(t);
        let t = strip_numbered(t);
        let mut l = strip_links(t);
        l = l.replace("**", "").replace("__", "").replace('`', "").replace('|', ", ");
        l = l.chars().filter(|c| *c != '*').collect();
        let l = l.trim();
        if !l.is_empty() {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(l);
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn strip_numbered(t: &str) -> &str {
    let digits = t.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && digits < 3 {
        if let Some(rest) = t[digits..].strip_prefix(". ").or_else(|| t[digits..].strip_prefix(") ")) {
            return rest;
        }
    }
    t
}

/// `[text](url)` → text; bare URLs → «ссылка».
fn strip_links(t: &str) -> String {
    let mut s = String::with_capacity(t.len());
    let mut rest = t;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        match (after.find("]("), after.find(']')) {
            (Some(close), Some(first_close)) if close == first_close => {
                let tail = &after[close + 2..];
                if let Some(end) = tail.find(')') {
                    s.push_str(&rest[..open]);
                    s.push_str(&after[..close]);
                    rest = &tail[end + 1..];
                    continue;
                }
                s.push_str(&rest[..open + 1]);
                rest = after;
            }
            _ => {
                s.push_str(&rest[..open + 1]);
                rest = after;
            }
        }
    }
    s.push_str(rest);
    s.split(' ')
        .map(|w| if w.starts_with("http://") || w.starts_with("https://") { "ссылка" } else { w })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_streamed_sentences() {
        let mut p = PhraseSplitter::default();
        assert!(p.push("Громкость теперь 30 проц").is_empty());
        assert_eq!(p.push("ентов. Калькулятор открыт"), vec!["Громкость теперь 30 процентов."]);
        assert_eq!(p.finish().as_deref(), Some("Калькулятор открыт"));
    }

    #[test]
    fn first_phrase_may_end_at_a_clause_but_later_ones_wait_for_the_sentence() {
        let mut p = PhraseSplitter::default();
        assert_eq!(p.push("Отличный вопрос, честно говоря, давно думаю об этом. "), vec!["Отличный вопрос, честно говоря,"]);
        assert!(p.push("Ну что ж, ").is_empty());
        assert_eq!(p.finish().as_deref(), Some("давно думаю об этом. Ну что ж,"));
    }

    #[test]
    fn first_clause_flushes_on_dash_colon_semicolon_at_24_chars_only_once() {
        let mut p = PhraseSplitter::default();
        // Short clause (<24 chars) holds back until >=24 chars.
        assert!(p.push("Да, конечно — ").is_empty());
        // Once >= 24 chars arrive and hit `:`, flushes immediately.
        assert_eq!(
            p.push("система проверена: память свободна, диски в норме; продолжаю мониторинг. "),
            vec![
                "Да, конечно — система проверена:",
                "память свободна, диски в норме; продолжаю мониторинг."
            ]
        );
        assert_eq!(p.finish(), None);
    }

    #[test]
    fn leading_blank_lines_do_not_consume_first_clause_fast_flush() {
        let mut p = PhraseSplitter::default();
        assert!(p.push("\n\n").is_empty());
        assert_eq!(
            p.push("Добрый день, я уже открыл нужную вкладку; всё готово"),
            vec!["Добрый день, я уже открыл нужную вкладку;"]
        );
        assert_eq!(p.finish().as_deref(), Some("всё готово"));
    }

    #[test]
    fn keeps_short_fragments_and_decimals_together() {
        let mut p = PhraseSplitter::default();
        assert!(p.push("Да. Готово. ").is_empty());
        assert_eq!(p.push("Значение 3.5 записано. "), vec!["Да. Готово. Значение 3.5 записано."]);
    }

    #[test]
    fn markdown_becomes_plain_speech() {
        let s = speakable("## Итог\n- **Громкость**: 30%\n1. Открыл [документацию](https://example.com)\n```\ncode\n```\nСм. https://x.y/z");
        assert_eq!(s, "Итог Громкость: 30% Открыл документацию См. ссылка");
    }
}
