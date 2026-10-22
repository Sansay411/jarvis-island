//! Russian text → what the synthesizer should pronounce.
//!
//! The model is asked to write spoken forms itself, but replies still carry
//! digits, symbols and Latin names now and then, and the synthesizer reads
//! «VS Code и» as «VS Codi». This pass makes the input unambiguous: numbers
//! in words with the right noun agreement, times, units, symbols, known
//! product names in Russian letters, abbreviations letter by letter.

/// Known names and terms, matched case-insensitively as whole words.
const NAMES: &[(&str, &str)] = &[
    ("vs code", "Ви Эс Код"),
    ("vscode", "Ви Эс Код"),
    ("visual studio code", "Вижуал Студио Код"),
    ("visual studio", "Вижуал Студио"),
    ("youtube", "Ютуб"),
    ("telegram", "Телеграм"),
    ("whatsapp", "Ватсап"),
    ("github", "Гитхаб"),
    ("gitlab", "Гитлаб"),
    ("git", "гит"),
    ("windows", "Виндоус"),
    ("google", "Гугл"),
    ("chrome", "Хром"),
    ("firefox", "Файрфокс"),
    ("edge", "Эдж"),
    ("yandex", "Яндекс"),
    ("docker", "Докер"),
    ("steam", "Стим"),
    ("blender", "Блендер"),
    ("discord", "Дискорд"),
    ("spotify", "Спотифай"),
    ("zoom", "Зум"),
    ("slack", "Слак"),
    ("teams", "Тимс"),
    ("word", "Ворд"),
    ("excel", "Эксель"),
    ("powerpoint", "Пауэрпойнт"),
    ("outlook", "Аутлук"),
    ("notepad", "Блокнот"),
    ("explorer", "Проводник"),
    ("terminal", "Терминал"),
    ("powershell", "Пауэршелл"),
    ("python", "Пайтон"),
    ("rust", "Раст"),
    ("javascript", "Джаваскрипт"),
    ("typescript", "Тайпскрипт"),
    ("react", "Реакт"),
    ("node", "Нод"),
    ("codex", "Кодекс"),
    ("claude code", "Клод Код"),
    ("claude", "Клод"),
    ("chatgpt", "Чат Джи Пи Ти"),
    ("openai", "Оупен Эй Ай"),
    ("jarvis", "Джарвис"),
    ("wi-fi", "вай-фай"),
    ("wifi", "вай-фай"),
    ("bluetooth", "блютус"),
    ("email", "имейл"),
    ("e-mail", "имейл"),
    ("ok", "окей"),
    ("okay", "окей"),
    ("online", "онлайн"),
    ("offline", "офлайн"),
    ("cursor", "Курсор"),
    ("obs", "О Би Эс"),
    ("alem", "Алем"),
    ("qwen", "Квен"),
];

const LETTERS: &[(char, &str)] = &[
    ('a', "эй"), ('b', "би"), ('c', "си"), ('d', "ди"), ('e', "и"), ('f', "эф"), ('g', "джи"), ('h', "эйч"),
    ('i', "ай"), ('j', "джей"), ('k', "кей"), ('l', "эл"), ('m', "эм"), ('n', "эн"), ('o', "оу"), ('p', "пи"),
    ('q', "кью"), ('r', "ар"), ('s', "эс"), ('t', "ти"), ('u', "ю"), ('v', "ви"), ('w', "дабл-ю"), ('x', "экс"),
    ('y', "уай"), ('z', "зед"),
];

#[derive(Clone, Copy, PartialEq)]
enum Gender {
    Masc,
    Fem,
    Neut,
}

const UNITS: [&str; 20] = [
    "ноль", "один", "два", "три", "четыре", "пять", "шесть", "семь", "восемь", "девять", "десять", "одиннадцать",
    "двенадцать", "тринадцать", "четырнадцать", "пятнадцать", "шестнадцать", "семнадцать", "восемнадцать",
    "девятнадцать",
];
const TENS: [&str; 10] = ["", "", "двадцать", "тридцать", "сорок", "пятьдесят", "шестьдесят", "семьдесят", "восемьдесят", "девяносто"];
const HUNDREDS: [&str; 10] = ["", "сто", "двести", "триста", "четыреста", "пятьсот", "шестьсот", "семьсот", "восемьсот", "девятьсот"];

/// 1 / 2–4 / 5+ form of a noun after a number.
fn plural<'a>(n: u64, one: &'a str, few: &'a str, many: &'a str) -> &'a str {
    let (d10, d100) = (n % 10, n % 100);
    if (11..=14).contains(&d100) {
        many
    } else if d10 == 1 {
        one
    } else if (2..=4).contains(&d10) {
        few
    } else {
        many
    }
}

fn triad(n: u64, g: Gender, out: &mut Vec<String>) {
    let (h, rest) = (n / 100, n % 100);
    if h > 0 {
        out.push(HUNDREDS[h as usize].into());
    }
    if rest == 0 {
        return;
    }
    let (t, u) = if rest < 20 { (0, rest) } else { (rest / 10, rest % 10) };
    if t > 0 {
        out.push(TENS[t as usize].into());
    }
    if u > 0 {
        out.push(
            match (u, g) {
                (1, Gender::Fem) => "одна",
                (1, Gender::Neut) => "одно",
                (2, Gender::Fem) => "две",
                _ => UNITS[u as usize],
            }
            .into(),
        );
    }
}

fn number_words(n: u64, g: Gender) -> String {
    if n == 0 {
        return "ноль".into();
    }
    let mut out = Vec::new();
    let scales: [(u64, Gender, [&str; 3]); 3] = [
        (1_000_000_000, Gender::Masc, ["миллиард", "миллиарда", "миллиардов"]),
        (1_000_000, Gender::Masc, ["миллион", "миллиона", "миллионов"]),
        (1_000, Gender::Fem, ["тысяча", "тысячи", "тысяч"]),
    ];
    let mut rest = n;
    for (size, sg, names) in scales {
        let k = rest / size;
        if k > 0 {
            triad(k, sg, &mut out);
            out.push(plural(k, names[0], names[1], names[2]).into());
            rest %= size;
        }
    }
    triad(rest, g, &mut out);
    out.join(" ")
}

/// Unit or noun after a number: (forms 1/2–4/5+, gender), matched by prefix.
fn unit_after(word: &str) -> Option<(&'static str, &'static str, &'static str, Gender, bool)> {
    let w = word.to_lowercase();
    let w = w.trim_end_matches(['.', ',', '!', '?', ';', ':']);
    // (forms, gender, replace the word itself — abbreviations are expanded)
    let table: &[(&[&str], &str, &str, &str, Gender, bool)] = &[
        (&["%"], "процент", "процента", "процентов", Gender::Masc, true),
        (&["мб", "mb"], "мегабайт", "мегабайта", "мегабайт", Gender::Masc, true),
        (&["гб", "gb"], "гигабайт", "гигабайта", "гигабайт", Gender::Masc, true),
        (&["кб", "kb"], "килобайт", "килобайта", "килобайт", Gender::Masc, true),
        (&["тб", "tb"], "терабайт", "терабайта", "терабайт", Gender::Masc, true),
        (&["мс", "ms"], "миллисекунда", "миллисекунды", "миллисекунд", Gender::Fem, true),
        (&["с", "сек", "s"], "секунда", "секунды", "секунд", Gender::Fem, true),
        (&["мин"], "минута", "минуты", "минут", Gender::Fem, true),
        (&["ч"], "час", "часа", "часов", Gender::Masc, true),
        (&["°c", "°с", "°"], "градус", "градуса", "градусов", Gender::Masc, true),
        (&["км"], "километр", "километра", "километров", Gender::Masc, true),
        (&["м"], "метр", "метра", "метров", Gender::Masc, true),
        (&["кг"], "килограмм", "килограмма", "килограммов", Gender::Masc, true),
        (&["₽", "руб"], "рубль", "рубля", "рублей", Gender::Masc, true),
        (&["$"], "доллар", "доллара", "долларов", Gender::Masc, true),
        (&["€"], "евро", "евро", "евро", Gender::Neut, true),
        (&["fps"], "кадр в секунду", "кадра в секунду", "кадров в секунду", Gender::Masc, true),
        (&["гц", "hz"], "герц", "герца", "герц", Gender::Masc, true),
    ];
    for (keys, one, few, many, g, replace) in table {
        if keys.contains(&w) {
            return Some((one, few, many, *g, *replace));
        }
    }
    None
}

/// Feminine and neuter nouns that commonly follow a number.
fn noun_gender(word: &str) -> Gender {
    let w = word.to_lowercase();
    const FEM: &[&str] = &[
        "минут", "секунд", "вкладк", "программ", "папк", "песн", "задач", "ссылк", "страниц", "недел", "штук",
        "ошибк", "строк", "запис", "заметк", "фотограф", "картин", "игр", "книг", "серии", "сери", "версии", "верси",
        "тысяч", "копи", "кнопк", "клавиш",
    ];
    const NEUT: &[&str] = &["окн", "сообщени", "приложени", "уведомлени", "письм", "видео", "изображени", "мест"];
    if FEM.iter().any(|f| w.starts_with(f)) {
        Gender::Fem
    } else if NEUT.iter().any(|f| w.starts_with(f)) {
        Gender::Neut
    } else {
        Gender::Masc
    }
}

fn is_latin_word(w: &str) -> bool {
    !w.is_empty() && w.chars().all(|c| c.is_ascii_alphabetic() || c == '-') && w.chars().any(|c| c.is_ascii_alphabetic())
}

/// Rough English → Russian letters for names not in the dictionary.
fn transliterate(w: &str) -> String {
    let lower = w.to_lowercase();
    let pairs: &[(&str, &str)] = &[
        ("sch", "ш"), ("tion", "шн"), ("sh", "ш"), ("ch", "ч"), ("th", "т"), ("ph", "ф"), ("ck", "к"), ("ee", "и"),
        ("oo", "у"), ("ea", "и"), ("ou", "ау"), ("qu", "кв"), ("wh", "в"), ("kh", "х"), ("zh", "ж"), ("ya", "я"),
        ("yu", "ю"), ("yo", "ё"),
    ];
    let single = |c: char| match c {
        'a' => "а", 'b' => "б", 'c' => "к", 'd' => "д", 'e' => "е", 'f' => "ф", 'g' => "г", 'h' => "х", 'i' => "и",
        'j' => "дж", 'k' => "к", 'l' => "л", 'm' => "м", 'n' => "н", 'o' => "о", 'p' => "п", 'q' => "к", 'r' => "р",
        's' => "с", 't' => "т", 'u' => "у", 'v' => "в", 'w' => "в", 'x' => "кс", 'y' => "и", 'z' => "з", '-' => "-",
        _ => "",
    };
    let mut out = String::new();
    let mut i = 0;
    let b = lower.as_str();
    'outer: while i < b.len() {
        for (from, to) in pairs {
            if b[i..].starts_with(from) {
                out.push_str(to);
                i += from.len();
                continue 'outer;
            }
        }
        let c = b[i..].chars().next().unwrap();
        // A final silent «e» (code, Adobe is rare enough to accept).
        if c == 'e' && i + 1 == b.len() && b.len() > 3 {
            i += 1;
            continue;
        }
        out.push_str(single(c));
        i += c.len_utf8();
    }
    out
}

fn spell_letters(w: &str) -> String {
    w.chars()
        .filter_map(|c| LETTERS.iter().find(|(l, _)| *l == c.to_ascii_lowercase()).map(|(_, n)| *n))
        .collect::<Vec<_>>()
        .join(" ")
}

fn replace_names(text: &str) -> String {
    let mut s = text.to_string();
    for (from, to) in NAMES {
        let lower = s.to_lowercase();
        let mut out = String::with_capacity(s.len());
        let mut last = 0;
        let mut search = 0;
        while let Some(pos) = lower[search..].find(from) {
            let start = search + pos;
            let end = start + from.len();
            let before = lower[..start].chars().next_back();
            let after = lower[end..].chars().next();
            let boundary = |c: Option<char>| c.is_none_or(|c| !c.is_alphanumeric());
            if boundary(before) && boundary(after) {
                out.push_str(&s[last..start]);
                out.push_str(to);
                last = end;
            }
            search = end;
        }
        out.push_str(&s[last..]);
        s = out;
    }
    s
}

/// Text for the synthesizer: everything spelled the way it is said.
pub fn normalize(text: &str) -> String {
    let text = replace_names(text)
        .replace('№', " номер ")
        .replace('&', " и ")
        .replace('+', " плюс ")
        .replace(['*', '#', '_', '`', '|'], " ")
        .replace('~', " около ")
        .replace(" / ", ", ")
        .replace('%', " %");
    // Split into tokens, keeping punctuation attached to words.
    let tokens: Vec<&str> = text.split_whitespace().collect();
    let mut out: Vec<String> = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        let tok = tokens[i];
        let core = tok.trim_end_matches(['.', ',', '!', '?', ';', ':', '…', ')', '»']);
        let tail = &tok[core.len()..];
        let core_lead = core.trim_start_matches(['(', '«']);
        let lead = &core[..core.len() - core_lead.len()];

        // Time 14:30 / 9:05.
        if let Some((h, m)) = core_lead.split_once(':') {
            if let (Ok(h), Ok(mm)) = (h.parse::<u64>(), m.parse::<u64>()) {
                if h < 24 && m.len() == 2 && mm < 60 {
                    let minutes = if mm == 0 {
                        "ноль ноль".to_string()
                    } else if mm < 10 {
                        format!("ноль {}", number_words(mm, Gender::Fem))
                    } else {
                        number_words(mm, Gender::Fem)
                    };
                    out.push(format!("{lead}{} {minutes}{tail}", number_words(h, Gender::Masc)));
                    i += 1;
                    continue;
                }
            }
        }
        // Decimal 3.5 / 3,5.
        if let Some((a, b)) = core_lead.split_once(['.', ',']) {
            if let (Ok(x), Ok(_)) = (a.parse::<u64>(), b.parse::<u64>()) {
                if !b.is_empty() && b.len() <= 3 {
                    let digits: Vec<String> = b.chars().map(|c| UNITS[c.to_digit(10).unwrap_or(0) as usize].to_string()).collect();
                    out.push(format!("{lead}{} и {}{tail}", number_words(x, Gender::Fem), digits.join(" ")));
                    i += 1;
                    continue;
                }
            }
        }
        // Integer (with optional thin spaces already split off) + unit/noun.
        if let Ok(n) = core_lead.parse::<u64>() {
            let next = tokens.get(i + 1).copied().unwrap_or("");
            if let Some((one, few, many, g, replace)) = unit_after(next) {
                let ntail = &next[next.trim_end_matches(['.', ',', '!', '?', ';', ':']).len()..];
                let noun = if replace { plural(n, one, few, many).to_string() } else { next.to_string() };
                out.push(format!("{lead}{}{tail} {noun}{ntail}", number_words(n, g)));
                i += 2;
                continue;
            }
            out.push(format!("{lead}{}{tail}", number_words(n, noun_gender(next))));
            i += 1;
            continue;
        }
        // Latin: abbreviations letter by letter, other words transliterated.
        if is_latin_word(core_lead) {
            let said = if core_lead.len() <= 5 && core_lead.chars().all(|c| c.is_ascii_uppercase()) {
                spell_letters(core_lead)
            } else {
                transliterate(core_lead)
            };
            out.push(format!("{lead}{said}{tail}"));
            i += 1;
            continue;
        }
        out.push(tok.to_string());
        i += 1;
    }
    out.join(" ").replace(" ,", ",").replace(" .", ".")
}

/// Pause after a phrase, by how it ends — sentences are synthesised one by
/// one, and without a gap they run together.
pub fn pause_after_ms(phrase: &str) -> u32 {
    match phrase.trim_end().chars().last() {
        Some('…') => 450,
        Some('?') | Some('!') => 320,
        Some('.') | Some(';') => 260,
        Some(',') | Some('—') | Some(':') => 140,
        _ => 200,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_in_words() {
        assert_eq!(number_words(0, Gender::Masc), "ноль");
        assert_eq!(number_words(21, Gender::Masc), "двадцать один");
        assert_eq!(number_words(1_252, Gender::Masc), "одна тысяча двести пятьдесят два");
        assert_eq!(number_words(2_000_001, Gender::Fem), "два миллиона одна");
        assert_eq!(number_words(512, Gender::Masc), "пятьсот двенадцать");
    }

    #[test]
    fn percent_and_units_agree() {
        assert_eq!(normalize("Громкость 28%."), "Громкость двадцать восемь процентов.");
        assert_eq!(normalize("Громкость 21 %"), "Громкость двадцать один процент");
        assert_eq!(normalize("Скачал 3 файла по 125 МБ."), "Скачал три файла по сто двадцать пять мегабайт.");
        assert_eq!(normalize("Осталась 1 минута"), "Осталась одна минута");
        assert_eq!(normalize("ещё 2 вкладки"), "ещё две вкладки");
        assert_eq!(normalize("за 384 мс"), "за триста восемьдесят четыре миллисекунды");
    }

    #[test]
    fn times_and_decimals() {
        assert_eq!(normalize("Сейчас 14:30."), "Сейчас четырнадцать тридцать.");
        assert_eq!(normalize("в 9:05"), "в девять ноль пять");
        assert_eq!(normalize("версия 3.5"), "версия три и пять");
    }

    #[test]
    fn names_and_latin() {
        assert_eq!(normalize("Открыл VS Code и YouTube."), "Открыл Ви Эс Код и Ютуб.");
        assert_eq!(normalize("Wi-Fi и Bluetooth включены"), "вай-фай и блютус включены");
        assert_eq!(normalize("порт USB"), "порт ю эс би");
        assert_eq!(normalize("Telegram, GitHub"), "Телеграм, Гитхаб");
        // Names inside other words stay.
        assert_eq!(normalize("Nodejs"), "нодеджс");
        assert_eq!(normalize("Привет, как дела?"), "Привет, как дела?");
    }

    #[test]
    fn pauses_follow_punctuation() {
        assert!(pause_after_ms("Ну что ж…") > pause_after_ms("Готово."));
        assert!(pause_after_ms("Правда?") > pause_after_ms("И ещё,"));
    }
}
