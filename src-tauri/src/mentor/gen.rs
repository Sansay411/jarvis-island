//! Hundreds of cards made by rule, so the feed never runs dry offline:
//! «что выведет код» with computed answers, logic and mental-math puzzles,
//! irregular verbs. Every answer is computed here, not written by hand, and
//! the wrong options are the mistakes people really make. The same seed
//! gives the same cards on every start, so ids stay put and spaced
//! repetition keeps working.

use std::collections::HashSet;

use serde_json::{json, Value};

/// `(id, track, kind, payload)`
pub type Card = (String, &'static str, &'static str, Value);

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn range(&mut self, a: i64, b: i64) -> i64 {
        a + (self.next() % ((b - a + 1) as u64)) as i64
    }
    fn pick<'a, T>(&mut self, v: &'a [T]) -> &'a T {
        &v[(self.next() % v.len() as u64) as usize]
    }
}

/// Four distinct options with the right one at a random place.
fn options(rng: &mut Rng, right: String, wrong: Vec<String>) -> Option<(Vec<String>, usize)> {
    let mut out: Vec<String> = Vec::new();
    for w in wrong {
        if w != right && !out.contains(&w) && !w.trim().is_empty() {
            out.push(w);
        }
    }
    if out.len() < 3 {
        return None;
    }
    out.truncate(3);
    let at = (rng.next() % 4) as usize;
    out.insert(at, right);
    Some((out, at))
}

struct Maker {
    cards: Vec<Card>,
    seen: HashSet<String>,
}

impl Maker {
    fn code(&mut self, rng: &mut Rng, id: String, title: &str, code: String, right: String, wrong: Vec<String>, explain: String) {
        if !self.seen.insert(code.clone()) {
            return;
        }
        if let Some((opts, answer)) = options(rng, right, wrong) {
            self.cards.push((id, "code", "code", json!({ "title": title, "code": code, "options": opts, "answer": answer, "explain": explain })));
        }
    }
    fn quiz(&mut self, rng: &mut Rng, id: String, track: &'static str, q: String, right: String, wrong: Vec<String>, explain: String) {
        if !self.seen.insert(q.clone()) {
            return;
        }
        if let Some((opts, answer)) = options(rng, right, wrong) {
            self.cards.push((id, track, "quiz", json!({ "q": q, "options": opts, "answer": answer, "explain": explain })));
        }
    }
}

const WORDS: &[&str] = &[
    "refactor", "database", "keyboard", "function", "compiler", "terminal", "pipeline", "variable", "debugger", "frontend", "endpoint", "template",
    "protocol", "monitor", "promise", "closure", "network", "storage",
];

/// Runs `f` until it has made `n` cards or tried too often.
fn each(m: &mut Maker, n: usize, seed: u64, mut f: impl FnMut(&mut Maker, &mut Rng, usize)) {
    let mut rng = Rng::new(seed);
    let start = m.cards.len();
    let mut i = 0;
    while m.cards.len() - start < n && i < n * 6 {
        f(m, &mut rng, i);
        i += 1;
    }
}

fn roman(mut n: i64) -> String {
    let table = [(90, "XC"), (50, "L"), (40, "XL"), (10, "X"), (9, "IX"), (5, "V"), (4, "IV"), (1, "I")];
    let mut s = String::new();
    for (v, r) in table {
        while n >= v {
            s.push_str(r);
            n -= v;
        }
    }
    s
}

/// Roman numerals the naive way: «IIII», «VIIII», «XXXX».
fn roman_additive(mut n: i64) -> String {
    let mut s = String::new();
    for (v, r) in [(50, "L"), (10, "X"), (5, "V"), (1, "I")] {
        while n >= v {
            s.push_str(r);
            n -= v;
        }
    }
    s
}

/// `(base, past, participle, перевод)`
const VERBS: &[(&str, &str, &str, &str)] = &[
    ("go", "went", "gone", "идти"), ("see", "saw", "seen", "видеть"), ("take", "took", "taken", "брать"), ("make", "made", "made", "делать"),
    ("know", "knew", "known", "знать"), ("think", "thought", "thought", "думать"), ("bring", "brought", "brought", "приносить"),
    ("buy", "bought", "bought", "покупать"), ("catch", "caught", "caught", "ловить"), ("teach", "taught", "taught", "учить"),
    ("write", "wrote", "written", "писать"), ("drive", "drove", "driven", "водить"), ("ride", "rode", "ridden", "ехать верхом"),
    ("speak", "spoke", "spoken", "говорить"), ("break", "broke", "broken", "ломать"), ("choose", "chose", "chosen", "выбирать"),
    ("forget", "forgot", "forgotten", "забывать"), ("get", "got", "got", "получать"), ("give", "gave", "given", "давать"),
    ("begin", "began", "begun", "начинать"), ("drink", "drank", "drunk", "пить"), ("swim", "swam", "swum", "плавать"),
    ("sing", "sang", "sung", "петь"), ("run", "ran", "run", "бегать"), ("come", "came", "come", "приходить"),
    ("become", "became", "become", "становиться"), ("eat", "ate", "eaten", "есть"), ("fall", "fell", "fallen", "падать"),
    ("feel", "felt", "felt", "чувствовать"), ("keep", "kept", "kept", "хранить"), ("leave", "left", "left", "уходить"),
    ("lose", "lost", "lost", "терять"), ("meet", "met", "met", "встречать"), ("pay", "paid", "paid", "платить"),
    ("say", "said", "said", "сказать"), ("sell", "sold", "sold", "продавать"), ("send", "sent", "sent", "отправлять"),
    ("spend", "spent", "spent", "тратить"), ("build", "built", "built", "строить"), ("understand", "understood", "understood", "понимать"),
    ("find", "found", "found", "находить"), ("hold", "held", "held", "держать"), ("lead", "led", "led", "вести"),
    ("put", "put", "put", "класть"), ("set", "set", "set", "устанавливать"), ("cut", "cut", "cut", "резать"), ("let", "let", "let", "позволять"),
    ("read", "read", "read", "читать"), ("show", "showed", "shown", "показывать"), ("throw", "threw", "thrown", "бросать"),
    ("grow", "grew", "grown", "расти"), ("draw", "drew", "drawn", "рисовать"), ("fly", "flew", "flown", "летать"),
    ("wear", "wore", "worn", "носить"), ("steal", "stole", "stolen", "красть"), ("hide", "hid", "hidden", "прятать"),
    ("bite", "bit", "bitten", "кусать"), ("shake", "shook", "shaken", "трясти"), ("wake", "woke", "woken", "просыпаться"),
];

/// A regular-looking past form, the mistake everyone makes once.
fn regularize(base: &str) -> String {
    if base.ends_with('e') {
        format!("{base}d")
    } else if base.ends_with('y') && !base.ends_with("ay") {
        format!("{}ied", &base[..base.len() - 1])
    } else {
        format!("{base}ed")
    }
}

pub fn cards() -> Vec<Card> {
    let mut m = Maker { cards: Vec::new(), seen: HashSet::new() };

    // ---------- code ----------
    each(&mut m, 25, 1, |m, r, i| {
        let (a, b) = (r.range(-29, -4), r.range(3, 9));
        let (js, py) = (a % b, a.rem_euclid(b));
        m.code(r, format!("gen-jsmod-{i}"), "Что выведет JavaScript?", format!("console.log({a} % {b})"), js.to_string(),
            vec![py.to_string(), (-py).to_string(), (a / b).to_string(), (-js).to_string()],
            format!("В JavaScript знак остатка как у делимого: {a} % {b} = {js}. В Python ответ был бы {py}: там знак как у делителя."));
    });
    each(&mut m, 25, 2, |m, r, i| {
        let (a, b) = (r.range(-29, -4), r.range(3, 9));
        let (div, md, tr, rm) = (a.div_euclid(b), a.rem_euclid(b), a / b, a % b);
        m.code(r, format!("gen-pydiv-{i}"), "Что выведет Python?", format!("print({a} // {b}, {a} % {b})"), format!("{div} {md}"),
            vec![format!("{tr} {rm}"), format!("{div} {rm}"), format!("{tr} {md}"), format!("{} {md}", div + 1)],
            format!("// округляет вниз, к минус бесконечности: {a} // {b} = {div}. Остаток берёт знак делителя: {md}. В C и JavaScript было бы {tr} и {rm}."));
    });
    each(&mut m, 30, 3, |m, r, i| {
        let w = *r.pick(WORDS);
        let i0 = r.range(0, 3) as usize;
        let j = r.range(i0 as i64 + 2, w.len() as i64 - 1) as usize;
        m.code(r, format!("gen-slice-{i}"), "Что выведет Python?", format!("print(\"{w}\"[{i0}:{j}])"), w[i0..j].to_string(),
            vec![w[i0..=j].to_string(), w[i0 + 1..=j].to_string(), w[i0 + 1..j].to_string(), w[..j].to_string()],
            format!("Срез [{i0}:{j}] берёт символы с номера {i0} по {} включительно, счёт с нуля. Правая граница не входит.", j - 1));
    });
    each(&mut m, 20, 4, |m, r, i| {
        let w = *r.pick(WORDS);
        let k = r.range(2, 4) as usize;
        let n = w.len();
        m.code(r, format!("gen-tail-{i}"), "Что выведет Python?", format!("print(\"{w}\"[-{k}:])"), w[n - k..].to_string(),
            vec![w[..k].to_string(), w[n - k - 1..].to_string(), w[n - k + 1..].to_string(), w[n - k..n - 1].to_string()],
            format!("Отрицательный индекс считает с конца: [-{k}:] это последние {k} символа."));
    });
    each(&mut m, 30, 5, |m, r, i| {
        let n = r.range(3, 6);
        let mut len = n;
        let mut lines = vec![format!("const a = [{}]", (1..=n).map(|x| x.to_string()).collect::<Vec<_>>().join(", "))];
        let mut has_concat = false;
        for _ in 0..r.range(3, 4) {
            match r.range(0, 4) {
                0 => { lines.push(format!("a.push({})", r.range(6, 9))); len += 1; }
                1 if len > 1 => { lines.push("a.pop()".into()); len -= 1; }
                2 if len > 1 => { lines.push("a.shift()".into()); len -= 1; }
                1 | 2 => { lines.push(format!("a.push({})", r.range(6, 9))); len += 1; }
                3 => { lines.push(format!("a.unshift({})", r.range(0, 9))); len += 1; }
                _ => { lines.push("a.concat([8, 9])".into()); has_concat = true; }
            }
        }
        lines.push("console.log(a.length)".into());
        let explain = if has_concat {
            "concat не меняет массив, а возвращает новый: его результат здесь выброшен. push и unshift добавляют, pop и shift убирают по одному.".to_string()
        } else {
            "push и unshift добавляют элемент, pop и shift убирают по одному. Посчитай шаги по порядку.".to_string()
        };
        m.code(r, format!("gen-jsarr-{i}"), "Что выведет JavaScript?", lines.join("\n"), len.to_string(),
            vec![(len + 2).to_string(), (len + 1).to_string(), (len - 1).to_string(), n.to_string()], explain);
    });
    each(&mut m, 40, 6, |m, r, i| {
        let (a, b) = (r.range(3, 15), r.range(3, 15));
        let (and, or, xor) = (a & b, a | b, a ^ b);
        let (op, right, name) = *r.pick(&[("&", and, "& оставляет единицы, общие для обоих"), ("|", or, "| ставит единицу, где она есть хоть у одного"), ("^", xor, "^ ставит единицу там, где биты разные")]);
        m.code(r, format!("gen-bits-{i}"), "Что выведет Python?", format!("print({a} {op} {b})"), right.to_string(),
            vec![and.to_string(), or.to_string(), xor.to_string(), (a + b).to_string(), (a * b).to_string()],
            format!("{a} = {a:04b}, {b} = {b:04b} в двоичном. {name}: {right:04b} = {right}."));
    });
    each(&mut m, 25, 7, |m, r, i| {
        let n = r.range(26, 255);
        let (hi, lo) = (n / 16, n % 16);
        m.quiz(r, format!("gen-hex-{i}"), "code", format!("Чему равно 0x{n:X} в десятичной системе?"), n.to_string(),
            vec![(lo * 16 + hi).to_string(), (n + 16).to_string(), (n - 16).to_string(), (hi * 10 + lo).to_string()],
            format!("0x{n:X} = {hi} × 16 + {lo} = {n}. Каждая шестнадцатеричная цифра это 4 бита."));
    });
    each(&mut m, 20, 8, |m, r, i| {
        let n = r.range(9, 63);
        m.quiz(r, format!("gen-bin-{i}"), "code", format!("Чему равно 0b{n:b} в десятичной системе?"), n.to_string(),
            vec![(n + 1).to_string(), (n - 1).to_string(), (n * 2).to_string(), (n / 2).to_string()],
            format!("Справа налево разряды стоят 1, 2, 4, 8, 16, 32. Сложи те, где единицы: {n:b} = {n}."));
    });
    each(&mut m, 30, 9, |m, r, i| {
        let (a, b) = (r.range(2, 9), r.range(2, 9));
        let forms: [(String, String, String); 4] = [
            (format!("\"{a}\" + {b}"), format!("\"{a}{b}\""), format!("Плюс со строкой склеивает: \"{a}\" + {b} = \"{a}{b}\".")),
            (format!("\"{a}\" - {b}"), (a - b).to_string(), format!("Минус бывает только числовым, поэтому строка стала числом: {}.", a - b)),
            (format!("\"{a}\" * \"{b}\""), (a * b).to_string(), format!("Умножение приводит обе строки к числам: {}.", a * b)),
            (format!("{a} + +\"{b}\""), (a + b).to_string(), format!("Унарный плюс превращает строку в число: {a} + {b} = {}.", a + b)),
        ];
        let (expr, right, explain) = r.pick(&forms).clone();
        m.code(r, format!("gen-coerce-{i}"), "Что выведет JavaScript?", format!("console.log({expr})"), right,
            vec![format!("\"{a}{b}\""), (a + b).to_string(), (a - b).to_string(), (a * b).to_string(), "NaN".into()], explain);
    });
    each(&mut m, 20, 10, |m, r, i| {
        let items: Vec<i64> = (0..r.range(6, 8)).map(|_| r.range(1, 5)).collect();
        let uniq = items.iter().collect::<HashSet<_>>().len();
        m.code(r, format!("gen-set-{i}"), "Что выведет Python?", format!("print(len(set({items:?})))"), uniq.to_string(),
            vec![items.len().to_string(), (uniq + 1).to_string(), (uniq.saturating_sub(1)).to_string(), "5".into()],
            "set оставляет только разные значения, повторы выбрасываются.".into());
    });
    each(&mut m, 25, 11, |m, r, i| {
        let (x, y) = (r.range(200, 250), r.range(20, 90));
        if x + y <= 255 {
            return;
        }
        m.code(r, format!("gen-wrap-{i}"), "Что напечатает Rust?", format!("let x: u8 = {x};\nprintln!(\"{{}}\", x.wrapping_add({y}));"), (x + y - 256).to_string(),
            vec![(x + y).to_string(), "255".into(), "паника: переполнение".into(), "0".into()],
            format!("u8 вмещает 0..=255. wrapping_add считает по кругу: {} − 256 = {}. Обычный + в debug-сборке здесь упал бы с паникой.", x + y, x + y - 256));
    });
    each(&mut m, 20, 12, |m, r, i| {
        let k = r.range(0, 9);
        let neg = i % 2 == 1;
        let x = if neg { format!("-{k}.5") } else { format!("{k}.5") };
        let even = if k % 2 == 0 { k } else { k + 1 };
        let other = if k % 2 == 0 { k + 1 } else { k };
        let (right, wrong) = if neg { (-even, -other) } else { (even, other) };
        m.code(r, format!("gen-round-{i}"), "Что выведет Python?", format!("print(round({x}))"), right.to_string(),
            vec![wrong.to_string(), x.clone(), (right + if neg { -2 } else { 2 }).to_string()],
            format!("Python округляет половинки к чётному («банковское» округление): round({x}) = {right}. Так меньше копится ошибка в суммах."));
    });
    each(&mut m, 18, 13, |m, r, i| {
        let n = r.range(4, 12);
        let up = i % 2 == 1;
        let (inner, right) = if up { (format!("range(i, {n})"), n * (n + 1) / 2) } else { ("range(i)".to_string(), n * (n - 1) / 2) };
        m.code(r, format!("gen-loops-{i}"), "Что выведет Python?", format!("count = 0\nfor i in range({n}):\n    for j in {inner}:\n        count += 1\nprint(count)"), right.to_string(),
            vec![(n * n).to_string(), (n * (n + 1) / 2).to_string(), (n * (n - 1) / 2).to_string(), (n * (n - 1)).to_string()],
            format!("Внутренний цикл каждый раз короче на один шаг: сумма 1 + 2 + … даёт {right}. Это всё ещё O(n²), просто вдвое меньше n²."));
    });
    each(&mut m, 15, 14, |m, r, i| {
        let n = r.range(10, 99);
        let unit = *r.pick(&["px", "em", "kg", "ms"]);
        m.code(r, format!("gen-parse-{i}"), "Что выведет JavaScript?", format!("console.log(parseInt(\"{n}{unit}\"), Number(\"{n}{unit}\"))"), format!("{n} NaN"),
            vec!["NaN NaN".into(), format!("{n} {n}"), format!("NaN {n}")],
            "parseInt читает цифры с начала и останавливается на первой букве. Number требует, чтобы вся строка была числом, иначе NaN.".into());
    });
    each(&mut m, 15, 15, |m, r, i| {
        let s = *r.pick(&["ab", "ha", "ok", "no", "go", "hi", "ya"]);
        let k = r.range(2, 4) as usize;
        m.code(r, format!("gen-mul-{i}"), "Что выведет Python?", format!("print(\"{s}\" * {k})"), s.repeat(k),
            vec![format!("{s}{k}"), s.repeat(k - 1), s.repeat(k + 1), "Ошибку".into()],
            format!("Строку можно умножить на число: она повторится {k} раза."));
    });

    // ---------- mind: logic and mental math ----------
    each(&mut m, 30, 20, |m, r, i| {
        let (a, d) = (r.range(2, 40), r.range(3, 17));
        let s: Vec<String> = (0..4).map(|k| (a + k * d).to_string()).collect();
        let right = a + 4 * d;
        m.quiz(r, format!("gen-arith-{i}"), "mind", format!("Какое число дальше: {}, ?", s.join(", ")), right.to_string(),
            vec![(right + d).to_string(), (right - 1).to_string(), (right + 1).to_string(), (a + 3 * d + d / 2).to_string()],
            format!("Каждый раз прибавляется {d}."));
    });
    each(&mut m, 15, 21, |m, r, i| {
        let (a, q) = (r.range(1, 6), r.range(2, 3));
        let s: Vec<String> = (0..4).map(|k| (a * q.pow(k as u32)).to_string()).collect();
        let right = a * q.pow(4);
        m.quiz(r, format!("gen-geom-{i}"), "mind", format!("Какое число дальше: {}, ?", s.join(", ")), right.to_string(),
            vec![(a * q.pow(3) + a * q.pow(3) / 2).to_string(), (right + q).to_string(), (right - a).to_string(), (a * q.pow(3) * (q + 1)).to_string()],
            format!("Каждое следующее в {q} раза больше."));
    });
    each(&mut m, 12, 22, |m, r, i| {
        let (a, b) = (r.range(1, 5), r.range(2, 7));
        let mut s = vec![a, b];
        for k in 2..6 {
            s.push(s[k - 1] + s[k - 2]);
        }
        let right = s[5];
        m.quiz(r, format!("gen-fib-{i}"), "mind", format!("Какое число дальше: {}, ?", s[..5].iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", ")), right.to_string(),
            vec![(right + 1).to_string(), (s[4] * 2).to_string(), (s[4] + s[2]).to_string(), (right - 1).to_string()],
            "Каждое число это сумма двух предыдущих, как у Фибоначчи.".into());
    });
    each(&mut m, 12, 23, |m, r, i| {
        let (a, up, down) = (r.range(5, 20), r.range(4, 9), r.range(1, 3));
        let mut s = vec![a];
        for k in 1..6 {
            let last = s[k - 1];
            s.push(if k % 2 == 1 { last + up } else { last - down });
        }
        let right = s[5];
        m.quiz(r, format!("gen-alt-{i}"), "mind", format!("Какое число дальше: {}, ?", s[..5].iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", ")), right.to_string(),
            vec![(s[4] - down).to_string(), (right + up).to_string(), (right + 1).to_string(), (s[4] + down).to_string()],
            format!("Шаги чередуются: плюс {up}, минус {down}."));
    });
    each(&mut m, 25, 24, |m, r, i| {
        let p = *r.pick(&[5, 10, 15, 20, 25, 30, 40, 50, 75]);
        let n = r.range(2, 30) * 20;
        let right = n * p / 100;
        m.quiz(r, format!("gen-pct-{i}"), "mind", format!("Сколько будет {p}% от {n}?"), right.to_string(),
            vec![(n * p / 10).to_string(), (n - right).to_string(), (right + 5).to_string(), (right * 2).to_string()],
            format!("{p}% это {p}/100. Удобно: 10% от {n} = {}, дальше умножай.", n / 10));
    });
    each(&mut m, 20, 25, |m, r, i| {
        let a = r.range(12, 98);
        if a % 10 == 0 {
            return;
        }
        let right = a * 11;
        m.quiz(r, format!("gen-x11-{i}"), "mind", format!("{a} × 11 = ? (в уме)"), right.to_string(),
            vec![(right + 10).to_string(), (right - 10).to_string(), (right + 100).to_string(), (a * 10 + 1).to_string()],
            format!("Трюк: раздвинь цифры {a} и вставь их сумму в середину, перенос добавь к первой цифре. {a} × 11 = {right}."));
    });
    each(&mut m, 9, 26, |m, r, i| {
        let n = 15 + 10 * i as i64;
        let t = n / 10;
        m.quiz(r, format!("gen-sq5-{i}"), "mind", format!("{n}² = ? (в уме)"), (n * n).to_string(),
            vec![(n * n + 10).to_string(), (t * t * 100 + 25).to_string(), (n * n - 100).to_string(), (n * 2 * 10).to_string()],
            format!("Число на 5 в квадрате: {t} × {} = {}, потом припиши 25. Получается {}.", t + 1, t * (t + 1), n * n));
    });
    each(&mut m, 20, 27, |m, r, i| {
        let h = r.range(1, 11);
        let mm = *r.pick(&[0, 30]);
        let raw = (30 * h - (11 * mm) / 2).abs();
        let angle = if raw > 180 { 360 - raw } else { raw };
        m.quiz(r, format!("gen-clock-{i}"), "mind", format!("Какой угол между стрелками часов в {h}:{mm:02}?"), format!("{angle}°"),
            vec![format!("{}°", (30 * h) % 360), format!("{}°", angle + 15), format!("{}°", (angle - 15).abs()), format!("{}°", 360 - angle)],
            "Часовая стрелка проходит 30° за час и 0,5° за минуту, минутная 6° за минуту. В :30 часовая уже на полпути к следующей цифре.".into());
    });
    each(&mut m, 20, 28, |m, r, i| {
        let (h1, m1) = (r.range(7, 11), r.range(0, 11) * 5);
        let (h2, m2) = (h1 + r.range(1, 4), r.range(0, 11) * 5);
        let right = (h2 * 60 + m2) - (h1 * 60 + m1);
        m.quiz(r, format!("gen-time-{i}"), "mind", format!("Сколько минут от {h1}:{m1:02} до {h2}:{m2:02}?"), right.to_string(),
            vec![(right + 60).to_string(), (right - 10).to_string(), ((h2 - h1) * 100 + m2 - m1).to_string(), (right + 5).to_string()],
            format!("Посчитай до ровного часа и дальше по часам: всего {} ч {} мин.", right / 60, right % 60));
    });
    each(&mut m, 25, 29, |m, r, i| {
        let n = r.range(4, 99);
        let right = roman(n);
        m.quiz(r, format!("gen-roman-{i}"), "mind", format!("Как записать {n} римскими цифрами?"), right.clone(),
            vec![roman_additive(n), roman(n + 1), roman(n - 1), roman(n + 10)],
            format!("{n} = {right}. Меньшая цифра перед большей вычитается: IV = 4, IX = 9, XL = 40, XC = 90. Больше трёх одинаковых подряд не пишут."));
    });

    // ---------- english: irregular verbs ----------
    for (i, (base, past, pp, ru)) in VERBS.iter().enumerate() {
        let mut r = Rng::new(100 + i as u64);
        let other = VERBS[(i + 7) % VERBS.len()];
        m.quiz(&mut r, format!("gen-past-{i}"), "english", format!("Past Simple от «{base}» ({ru})?"), past.to_string(),
            vec![regularize(base), pp.to_string(), base.to_string(), other.1.to_string()],
            format!("{base} → {past} → {pp}. Неправильные глаголы учат тройками, вслух: так ритм запоминается быстрее."));
        m.quiz(&mut r, format!("gen-pp-{i}"), "english", format!("Третья форма (Past Participle) от «{base}»?"), pp.to_string(),
            vec![past.to_string(), regularize(base), base.to_string(), other.2.to_string()],
            format!("{base} → {past} → {pp}. Третья форма нужна для Present Perfect: I have {pp}."));
    }

    m.cards
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn makes_hundreds_of_valid_cards_with_stable_ids() {
        let a = cards();
        assert!(a.len() >= 500, "only {} cards", a.len());
        let ids: HashSet<&str> = a.iter().map(|c| c.0.as_str()).collect();
        assert_eq!(ids.len(), a.len(), "duplicate ids");
        for (id, _, kind, v) in &a {
            assert!(super::super::valid_card(kind, v), "bad card {id}: {v}");
            let opts = v["options"].as_array().unwrap();
            let set: HashSet<&str> = opts.iter().map(|o| o.as_str().unwrap()).collect();
            assert_eq!(set.len(), 4, "repeated options in {id}");
        }
        assert_eq!(cards()[123].3, a[123].3, "same seed, same cards");
    }

    #[test]
    fn computed_answers_are_right() {
        let find = |prefix: &str| cards().into_iter().find(|c| c.0.starts_with(prefix)).unwrap().3;
        let pick = |v: &Value| v["options"][v["answer"].as_u64().unwrap() as usize].as_str().unwrap().to_string();
        let v = find("gen-jsmod-");
        let code = v["code"].as_str().unwrap();
        let nums: Vec<i64> = code.trim_start_matches("console.log(").trim_end_matches(')').split(" % ").map(|x| x.parse().unwrap()).collect();
        assert_eq!(pick(&v), (nums[0] % nums[1]).to_string());
        assert_eq!(roman(94), "XCIV");
        assert_eq!(roman(49), "XLIX");
        assert_eq!(roman_additive(9), "VIIII");
        assert_eq!(regularize("drive"), "drived");
        assert_eq!(regularize("fly"), "flied");
    }
}
