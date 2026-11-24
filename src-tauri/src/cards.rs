//! Explanation cards: data model, tolerant validation and clamping.
//! Mirrors `src/overlay/cards/types.ts` and `src/overlay/cards/normalize.ts`;
//! keep the two in step. Model output is untrusted: numbers come as strings,
//! kinds get other names, arrays arrive stringified or as tuples. Everything
//! usable is kept, everything else is dropped, every list and string is capped.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::error::{AppError, AppResult};

pub const MAX_TITLE: usize = 120;
pub const MAX_SUBTITLE: usize = 200;
pub const MAX_BLOCKS: usize = 8;
pub const MAX_TEXT: usize = 1200;
pub const MAX_SHORT_TEXT: usize = 240;
pub const MAX_CELL_TEXT: usize = 120;
pub const MAX_STEPS: usize = 10;
pub const MAX_TABLE_COLS: usize = 6;
pub const MAX_TABLE_ROWS: usize = 12;
pub const MAX_BARS: usize = 10;
pub const MAX_LINE_SERIES: usize = 4;
pub const MAX_LINE_POINTS: usize = 16;
pub const MAX_TIMELINE: usize = 10;
pub const MAX_FLOW_NODES: usize = 12;
pub const MAX_FLOW_EDGES: usize = 20;
pub const MAX_COMPARE_COLS: usize = 3;
pub const MAX_COMPARE_ITEMS: usize = 8;
pub const MAX_MATH_LINES: usize = 10;
pub const MAX_QUIZ_OPTIONS: usize = 6;
pub const MAX_KV_ITEMS: usize = 12;
/// Whole card as JSON, in characters: trailing blocks go first past this.
pub const MAX_CARD_JSON: usize = 12_000;
/// Raw blocks looked at, at most.
const MAX_RAW_BLOCKS: usize = 32;
/// Title used when the model forgot one but the blocks are fine.
pub const FALLBACK_TITLE: &str = "Пояснение";

/// Physical screen-pixel rectangle the overlay places a card beside.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnchorRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardSpec {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Block {
    Text {
        text: String,
    },
    Steps {
        items: Vec<StepItem>,
    },
    #[serde(rename_all = "camelCase")]
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        highlight_col: Option<usize>,
    },
    Bars {
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        unit: Option<String>,
        items: Vec<BarItem>,
    },
    #[serde(rename_all = "camelCase")]
    Line {
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        unit: Option<String>,
        x_labels: Vec<String>,
        series: Vec<LineSeries>,
    },
    Timeline {
        items: Vec<TimelineItem>,
    },
    Flow {
        #[serde(skip_serializing_if = "Option::is_none")]
        direction: Option<String>,
        nodes: Vec<FlowNode>,
        edges: Vec<FlowEdge>,
    },
    Compare {
        columns: Vec<CompareColumn>,
    },
    Math {
        lines: Vec<MathLine>,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<String>,
    },
    Definition {
        term: String,
        meaning: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        example: Option<String>,
    },
    Quiz {
        question: String,
        options: Vec<String>,
        correct: usize,
        #[serde(skip_serializing_if = "Option::is_none")]
        explanation: Option<String>,
    },
    Callout {
        tone: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        text: String,
    },
    Kv {
        items: Vec<KvItem>,
    },
}

impl Block {
    pub fn kind(&self) -> &'static str {
        match self {
            Block::Text { .. } => "text",
            Block::Steps { .. } => "steps",
            Block::Table { .. } => "table",
            Block::Bars { .. } => "bars",
            Block::Line { .. } => "line",
            Block::Timeline { .. } => "timeline",
            Block::Flow { .. } => "flow",
            Block::Compare { .. } => "compare",
            Block::Math { .. } => "math",
            Block::Definition { .. } => "definition",
            Block::Quiz { .. } => "quiz",
            Block::Callout { .. } => "callout",
            Block::Kv { .. } => "kv",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepItem {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BarItem {
    pub label: String,
    pub value: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub highlight: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LineSeries {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub values: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimelineItem {
    pub date: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlowNode {
    pub id: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tone: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlowEdge {
    pub from: String,
    pub to: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompareItem {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompareColumn {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accent: Option<bool>,
    pub items: Vec<CompareItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MathLine {
    pub expr: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KvItem {
    pub key: String,
    pub value: String,
}

type Obj = Map<String, Value>;

// ---------- primitives ----------

/// Drops control characters (keeps new lines and tabs) and trims.
fn clean_text(s: &str) -> String {
    let unified = s.replace("\r\n", "\n").replace('\r', "\n");
    let kept: String = unified.chars().filter(|c| *c == '\n' || *c == '\t' || !c.is_control()).collect();
    kept.trim().to_string()
}

fn clamp_str(v: Option<&Value>, max: usize) -> String {
    let raw = match v {
        Some(Value::String(s)) => clean_text(s),
        Some(Value::Number(n)) => match n.as_f64() {
            Some(f) if f.is_finite() => n.to_string(),
            _ => return String::new(),
        },
        Some(Value::Bool(b)) => if *b { "Да" } else { "Нет" }.to_string(),
        _ => return String::new(),
    };
    if raw.chars().count() <= max {
        return raw;
    }
    let head: String = raw.chars().take(max.saturating_sub(1).max(1)).collect();
    format!("{}…", head.trim_end())
}

/// First non-empty string among `keys`.
fn pick(o: &Obj, keys: &[&str], max: usize) -> String {
    keys.iter().map(|k| clamp_str(o.get(*k), max)).find(|s| !s.is_empty()).unwrap_or_default()
}

fn opt_pick(o: &Obj, keys: &[&str], max: usize) -> Option<String> {
    Some(pick(o, keys, max)).filter(|s| !s.is_empty())
}

/// First value among `keys` that is present and not null.
fn pick_raw<'a>(o: &'a Obj, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|k| o.get(*k).filter(|v| !v.is_null()))
}

/// Arrays pass through; a string holding a JSON array is parsed.
fn as_arr(v: Option<&Value>) -> Vec<Value> {
    match v {
        Some(Value::Array(a)) => a.clone(),
        Some(Value::String(s)) if s.trim_start().starts_with('[') => match serde_json::from_str::<Value>(s.trim()) {
            Ok(Value::Array(a)) => a,
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

/// First non-empty array among `keys`.
fn pick_arr(o: &Obj, keys: &[&str]) -> Vec<Value> {
    keys.iter().map(|k| as_arr(o.get(*k))).find(|a| !a.is_empty()).unwrap_or_default()
}

/// A plain object used as a map (`{"Порт": 443}`) as pairs. serde_json keeps
/// such maps sorted by key, so the arrays the schema asks for keep order better.
fn map_entries(v: Option<&Value>) -> Vec<(String, Value)> {
    match v {
        Some(Value::Object(m)) => m.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        _ => Vec::new(),
    }
}

/// Reads a number the way people write it: `42`, `"42,5"`, `"1 200 ₽"`,
/// `"$1,234.5"`, `"−7 %"`. The first number in a string wins; a lone comma
/// is a decimal mark unless exactly three digits follow it (`"1,500"` = 1500).
pub fn parse_number(v: Option<&Value>) -> Option<f64> {
    match v? {
        Value::Number(n) => n.as_f64().filter(|f| f.is_finite()),
        Value::String(s) => parse_number_str(s),
        _ => None,
    }
}

fn parse_number_str(s: &str) -> Option<f64> {
    let chars: Vec<char> = s
        .chars()
        .map(|c| if matches!(c, '\u{2212}' | '\u{2012}' | '\u{2013}' | '\u{2014}') { '-' } else { c })
        .collect();
    let start = chars.iter().position(|c| c.is_ascii_digit())?;
    let negative = start > 0 && chars[start - 1] == '-';
    let mut t = String::new();
    for &c in &chars[start..] {
        if c.is_ascii_digit() || c == '.' || c == ',' {
            t.push(c);
        } else if c.is_whitespace() || c == '\'' {
            continue;
        } else {
            break;
        }
    }
    let t = t.trim_end_matches(['.', ',']);
    let dots = t.matches('.').count();
    let commas = t.matches(',').count();
    let normalized = if dots > 0 && commas > 0 {
        let comma_decimal = t.rfind(',') > t.rfind('.');
        if comma_decimal { t.replace('.', "").replacen(',', ".", 1) } else { t.replace(',', "") }
    } else if commas > 1 {
        t.replace(',', "")
    } else if commas == 1 {
        let (int, frac) = t.split_once(',').unwrap_or((t, ""));
        if frac.len() == 3 && int != "0" { format!("{int}{frac}") } else { format!("{int}.{frac}") }
    } else if dots > 1 {
        t.replace('.', "")
    } else {
        t.to_string()
    };
    let n: f64 = normalized.parse().ok()?;
    let n = if negative { -n } else { n };
    n.is_finite().then_some(n)
}

/// A number, but never from a boolean.
fn num(v: Option<&Value>) -> Option<f64> {
    if matches!(v, Some(Value::Bool(_))) {
        return None;
    }
    parse_number(v)
}

fn to_bool(v: Option<&Value>) -> bool {
    match v {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64() == Some(1.0),
        Some(Value::String(s)) => matches!(s.trim().to_lowercase().as_str(), "true" | "yes" | "да" | "1"),
        _ => false,
    }
}

/// Lower case without spaces, `_` and `-`: `"Key_Value"` → `"keyvalue"`.
fn key(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace() && *c != '_' && *c != '-').flat_map(char::to_lowercase).collect()
}

// ---------- kinds ----------

fn kind_alias(alias: &str) -> Option<&'static str> {
    Some(match alias {
        "text" | "markdown" | "md" | "paragraph" | "summary" => "text",
        "steps" | "step" | "list" | "process" | "howto" | "instructions" | "checklist" => "steps",
        "table" | "grid" => "table",
        "bars" | "bar" | "barchart" | "histogram" => "bars",
        "line" | "linechart" | "trend" => "line",
        "timeline" | "chronology" | "history" => "timeline",
        "flow" | "flowchart" | "diagram" => "flow",
        "compare" | "comparison" | "versus" | "vs" | "proscons" => "compare",
        "math" | "formula" | "equation" | "solution" | "calculation" => "math",
        "definition" | "define" | "term" | "glossary" => "definition",
        "quiz" | "question" | "test" => "quiz",
        "callout" | "tip" | "hint" | "note" | "idea" | "info" | "warning" | "warn" | "alert" | "important" => "callout",
        "kv" | "keyvalue" | "facts" | "specs" | "properties" | "stats" => "kv",
        _ => return None,
    })
}

/// Guesses the kind from the fields when the name is missing or unknown.
fn infer_kind(o: &Obj) -> Option<&'static str> {
    let has = |k: &str| o.get(k).is_some_and(|v| !v.is_null());
    if has("nodes") || has("edges") {
        return Some("flow");
    }
    if has("question") && (has("options") || has("choices") || has("answers")) {
        return Some("quiz");
    }
    if has("headers") || has("rows") {
        return Some("table");
    }
    if has("series") || has("xLabels") || has("x_labels") || has("points") {
        return Some("line");
    }
    if has("term") && (has("meaning") || has("definition")) {
        return Some("definition");
    }
    if has("lines") || has("expr") || has("formula") {
        return Some("math");
    }
    if has("columns") {
        return Some("compare");
    }
    let items = pick_arr(o, &["items", "bars", "data", "events", "pairs", "steps"]);
    if let Some(Value::Object(first)) = items.first() {
        if pick_raw(first, &["date", "year", "time", "when"]).is_some() {
            return Some("timeline");
        }
        if let Some(val) = pick_raw(first, &["value", "val", "y", "count", "amount"]) {
            return Some(if num(Some(val)).is_some() { "bars" } else { "kv" });
        }
        if pick_raw(first, &["key", "k"]).is_some() {
            return Some("kv");
        }
        return Some("steps");
    }
    if !items.is_empty() {
        return Some("steps");
    }
    if has("tone") || has("variant") {
        return Some("callout");
    }
    if has("text") || has("content") || has("body") || has("markdown") {
        return Some("text");
    }
    None
}

// ---------- blocks ----------

const TITLE_KEYS: &[&str] = &["title", "label", "name", "text", "heading", "step"];
const NOTE_KEYS: &[&str] = &["note", "detail", "details", "description", "sub", "subtitle", "desc"];

fn norm_text(o: &Obj) -> Option<Block> {
    let mut text = pick(o, &["text", "content", "body", "markdown", "value", "message"], MAX_TEXT);
    if text.is_empty() {
        let lines: Vec<String> = pick_arr(o, &["items", "lines", "bullets", "points"])
            .iter()
            .map(|it| clamp_str(Some(it), MAX_SHORT_TEXT))
            .filter(|s| !s.is_empty())
            .map(|s| format!("- {s}"))
            .collect();
        text = clamp_str(Some(&Value::String(lines.join("\n"))), MAX_TEXT);
    }
    (!text.is_empty()).then_some(Block::Text { text })
}

fn norm_steps(o: &Obj) -> Option<Block> {
    let mut items = Vec::new();
    for it in pick_arr(o, &["items", "steps", "list", "points"]) {
        if items.len() >= MAX_STEPS {
            break;
        }
        if let Value::Object(m) = &it {
            let title = pick(m, TITLE_KEYS, MAX_SHORT_TEXT);
            if title.is_empty() {
                continue;
            }
            let note = opt_pick(m, NOTE_KEYS, MAX_SHORT_TEXT).filter(|n| *n != title);
            items.push(StepItem { title, note });
        } else {
            let title = clamp_str(Some(&it), MAX_SHORT_TEXT);
            if !title.is_empty() {
                items.push(StepItem { title, note: None });
            }
        }
    }
    (!items.is_empty()).then_some(Block::Steps { items })
}

/// A table row given as an array, or as `"a | b | c"`.
fn row_arr(r: &Value) -> Option<Vec<Value>> {
    match r {
        Value::Array(a) => Some(a.clone()),
        Value::String(s) if s.contains('|') => {
            let parts: Vec<&str> = s.split('|').map(str::trim).collect();
            let last = parts.len().saturating_sub(1);
            Some(
                parts
                    .iter()
                    .enumerate()
                    .filter(|(i, c)| !c.is_empty() || (*i > 0 && *i < last))
                    .map(|(_, c)| Value::String((*c).to_string()))
                    .collect(),
            )
        }
        Value::String(s) => Some(vec![Value::String(s.clone())]),
        _ => None,
    }
}

fn norm_table(o: &Obj) -> Option<Block> {
    let raw_rows = pick_arr(o, &["rows", "data", "items"]);
    let mut headers: Vec<String> = pick_arr(o, &["headers", "columns", "cols", "header"])
        .iter()
        .map(|h| match h {
            Value::Object(m) => clamp_str(pick_raw(m, &["title", "label", "name", "text"]), MAX_CELL_TEXT),
            other => clamp_str(Some(other), MAX_CELL_TEXT),
        })
        .collect();
    let named = |hs: &[String]| hs.iter().any(|h| !h.is_empty());
    if !named(&headers) {
        if let Some(Value::Object(first)) = raw_rows.iter().find(|r| r.is_object()) {
            headers = first.keys().map(|k| clamp_str(Some(&Value::String(k.clone())), MAX_CELL_TEXT)).collect();
        }
    }
    let widest = raw_rows.iter().filter_map(row_arr).map(|r| r.len()).max().unwrap_or(0);
    let cols = headers.len().max(if named(&headers) { 0 } else { widest }).min(MAX_TABLE_COLS);
    if cols == 0 {
        return None;
    }
    headers.truncate(cols);
    headers.resize(cols, String::new());

    let mut rows = Vec::new();
    for r in &raw_rows {
        if rows.len() >= MAX_TABLE_ROWS {
            break;
        }
        let cells: Vec<String> = if let Value::Object(m) = r {
            let values: Vec<&Value> = m.values().collect();
            headers
                .iter()
                .enumerate()
                .map(|(ci, h)| {
                    let by_name = (!h.is_empty()).then(|| m.iter().find(|(k, _)| key(k) == key(h)).map(|(_, v)| v)).flatten();
                    clamp_str(by_name.or_else(|| values.get(ci).copied()), MAX_CELL_TEXT)
                })
                .collect()
        } else {
            let Some(arr) = row_arr(r) else { continue };
            (0..cols).map(|ci| clamp_str(arr.get(ci), MAX_CELL_TEXT)).collect()
        };
        if cells.iter().any(|c| !c.is_empty()) {
            rows.push(cells);
        }
    }
    if rows.is_empty() {
        return None;
    }

    let hl_raw = pick_raw(o, &["highlightCol", "highlight_col", "highlight", "highlightColumn"]);
    let hl_num = num(hl_raw);
    let highlight_col = match (hl_num, hl_raw) {
        (Some(n), _) => (n.fract() == 0.0 && n >= 0.0 && (n as usize) < cols).then_some(n as usize),
        (None, Some(Value::String(s))) => headers.iter().position(|h| !h.is_empty() && key(h) == key(s)),
        _ => None,
    };
    Some(Block::Table { headers, rows, highlight_col })
}

fn norm_bars(o: &Obj) -> Option<Block> {
    let mut items: Vec<BarItem> = Vec::new();
    let mut push = |label: Option<&Value>, value: Option<&Value>, hl: Option<&Value>| {
        if items.len() >= MAX_BARS {
            return;
        }
        let label = clamp_str(label, MAX_CELL_TEXT);
        let Some(value) = num(value) else { return };
        if label.is_empty() {
            return;
        }
        items.push(BarItem { label, value, highlight: to_bool(hl).then_some(true) });
    };
    let src = pick_arr(o, &["items", "bars", "data", "points"]);
    if !src.is_empty() {
        for it in &src {
            match it {
                Value::Object(m) => push(
                    pick_raw(m, &["label", "name", "key", "x", "title"]),
                    pick_raw(m, &["value", "val", "y", "count", "amount"]),
                    pick_raw(m, &["highlight", "accent"]),
                ),
                Value::Array(a) => push(a.first(), a.get(1), a.get(2)),
                _ => {}
            }
        }
    } else {
        let labels = pick_arr(o, &["labels", "xLabels", "x_labels", "categories"]);
        let values = as_arr(o.get("values"));
        if !labels.is_empty() && !values.is_empty() {
            for (i, l) in labels.iter().enumerate() {
                push(Some(l), values.get(i), None);
            }
        } else {
            for (k, v) in map_entries(pick_raw(o, &["data", "values", "items"])) {
                push(Some(&Value::String(k)), Some(&v), None);
            }
        }
    }
    if items.is_empty() {
        return None;
    }
    Some(Block::Bars {
        title: opt_pick(o, &["title", "label", "caption"], MAX_CELL_TEXT),
        unit: opt_pick(o, &["unit", "units", "suffix"], 24),
        items,
    })
}

/// Numbers from a list of values, `{x, y}` points or `[x, y]` pairs; point
/// labels go to `labels` when given.
fn numbers(src: &[Value], mut labels: Option<&mut Vec<String>>) -> Vec<f64> {
    let mut out = Vec::new();
    for v in src {
        if out.len() >= MAX_LINE_POINTS {
            break;
        }
        let (n, label) = match v {
            Value::Object(m) => (num(pick_raw(m, &["y", "value", "val"])), Some(pick_raw(m, &["x", "label", "name"]))),
            Value::Array(a) => (num(a.get(1)), Some(a.first())),
            other => (num(Some(other)), None),
        };
        let Some(n) = n else { continue };
        if let (Some(labels), Some(label)) = (labels.as_deref_mut(), label) {
            let s = clamp_str(label, 24);
            labels.push(if s.is_empty() { (out.len() + 1).to_string() } else { s });
        }
        out.push(n);
    }
    out
}

fn norm_line(o: &Obj) -> Option<Block> {
    let mut x_labels: Vec<String> = pick_arr(o, &["xLabels", "x_labels", "labels", "categories", "x"])
        .iter()
        .take(MAX_LINE_POINTS)
        .enumerate()
        .map(|(i, l)| Some(clamp_str(Some(l), 24)).filter(|s| !s.is_empty()).unwrap_or_else(|| (i + 1).to_string()))
        .collect();
    let mut series: Vec<LineSeries> = Vec::new();
    let mut point_labels: Vec<String> = Vec::new();

    let raw_series = pick_arr(o, &["series", "datasets", "lines"]);
    if !raw_series.is_empty() {
        for s in &raw_series {
            if series.len() >= MAX_LINE_SERIES {
                break;
            }
            let mut labels = Vec::new();
            let (values, name) = match s {
                Value::Object(m) => (
                    numbers(&pick_arr(m, &["values", "points", "data", "y"]), Some(&mut labels)),
                    opt_pick(m, &["name", "label", "title"], 48),
                ),
                other => (numbers(&as_arr(Some(other)), None), None),
            };
            if values.len() < 2 {
                continue;
            }
            if point_labels.is_empty() {
                point_labels = labels;
            }
            series.push(LineSeries { name, values });
        }
    } else {
        let values = numbers(&pick_arr(o, &["points", "values", "data", "y"]), Some(&mut point_labels));
        if values.len() >= 2 {
            series.push(LineSeries { name: None, values });
        }
    }
    if series.is_empty() {
        return None;
    }
    if x_labels.is_empty() {
        x_labels = point_labels;
    }
    let max_len = series.iter().map(|s| s.values.len()).max().unwrap_or(0);
    x_labels.truncate(max_len);
    while x_labels.len() < max_len {
        x_labels.push((x_labels.len() + 1).to_string());
    }
    Some(Block::Line {
        title: opt_pick(o, &["title", "label", "caption"], MAX_CELL_TEXT),
        unit: opt_pick(o, &["unit", "units", "suffix"], 24),
        x_labels,
        series,
    })
}

fn norm_timeline(o: &Obj) -> Option<Block> {
    let mut items = Vec::new();
    for it in pick_arr(o, &["items", "events", "timeline", "points"]) {
        if items.len() >= MAX_TIMELINE {
            break;
        }
        let Value::Object(m) = &it else { continue };
        let date = pick(m, &["date", "year", "time", "when", "period"], 40);
        let mut label = pick(m, &["label", "title", "event", "name", "text"], MAX_SHORT_TEXT);
        let mut note = opt_pick(m, &["note", "detail", "details", "description", "desc"], MAX_SHORT_TEXT);
        if label.is_empty() {
            if let Some(n) = note.take() {
                label = n;
            }
        }
        if date.is_empty() || label.is_empty() {
            continue;
        }
        let note = note.filter(|n| *n != label);
        items.push(TimelineItem { date, label, note });
    }
    (!items.is_empty()).then_some(Block::Timeline { items })
}

fn flow_tone(m: &Obj) -> Option<String> {
    let t = key(&clamp_str(pick_raw(m, &["tone", "style", "variant"]), 16));
    if matches!(t.as_str(), "accent" | "highlight" | "primary" | "key" | "main") || to_bool(m.get("highlight")) || to_bool(m.get("accent")) {
        return Some("accent".into());
    }
    matches!(t.as_str(), "muted" | "secondary" | "dim" | "optional" | "faded").then(|| "muted".into())
}

fn flow_direction(v: Option<&Value>) -> Option<String> {
    let d = key(&clamp_str(v, 16));
    match d.as_str() {
        "lr" | "rl" | "leftright" | "lefttoright" | "horizontal" | "row" | "x" => Some("lr".into()),
        "td" | "tb" | "bt" | "topdown" | "toptobottom" | "topbottom" | "vertical" | "column" | "y" => Some("td".into()),
        _ => None,
    }
}

/// `"a -> b"`, `"a --> b"`, `"a → b"`, `"a => b"` as endpoints.
fn split_arrows(s: &str) -> Vec<String> {
    let cs: Vec<char> = s.chars().collect();
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        let arrow_end = match c {
            '→' | '⟶' => Some(i + 1),
            '-' | '=' => {
                let mut j = i;
                while j < cs.len() && cs[j] == c {
                    j += 1;
                }
                (cs.get(j) == Some(&'>')).then_some(j + 1)
            }
            _ => None,
        };
        if let Some(end) = arrow_end {
            parts.push(cur.trim().to_string());
            cur.clear();
            i = end;
        } else {
            cur.push(c);
            i += 1;
        }
    }
    parts.push(cur.trim().to_string());
    parts
}

fn norm_flow(o: &Obj) -> Option<Block> {
    let mut nodes: Vec<FlowNode> = Vec::new();
    let mut ids: HashSet<String> = HashSet::new();
    fn add(nodes: &mut Vec<FlowNode>, ids: &mut HashSet<String>, id: String, label: String, note: Option<String>, tone: Option<String>) -> bool {
        if id.is_empty() || label.is_empty() || ids.contains(&id) || nodes.len() >= MAX_FLOW_NODES {
            return false;
        }
        ids.insert(id.clone());
        let note = note.filter(|n| *n != label);
        nodes.push(FlowNode { id, label, note, tone });
        true
    }

    for n in pick_arr(o, &["nodes", "steps", "items"]) {
        if let Value::Object(m) = &n {
            let label = pick(m, &["label", "title", "text", "name"], MAX_CELL_TEXT);
            let mut id = clamp_str(pick_raw(m, &["id", "key"]), 48);
            if id.is_empty() {
                id = clamp_str(Some(&Value::String(label.clone())), 48);
            }
            let label = if label.is_empty() { clamp_str(Some(&Value::String(id.clone())), MAX_CELL_TEXT) } else { label };
            let note = opt_pick(m, &["note", "sub", "detail", "description"], MAX_CELL_TEXT);
            add(&mut nodes, &mut ids, id, label, note, flow_tone(m));
        } else {
            let label = clamp_str(Some(&n), MAX_CELL_TEXT);
            let id = clamp_str(Some(&Value::String(label.clone())), 48);
            add(&mut nodes, &mut ids, id, label, None, None);
        }
    }
    let declared = !nodes.is_empty();

    let mut edges: Vec<FlowEdge> = Vec::new();
    let mut seen_edges: HashSet<(String, String, String)> = HashSet::new();
    for e in pick_arr(o, &["edges", "links", "connections", "arrows"]) {
        if edges.len() >= MAX_FLOW_EDGES {
            break;
        }
        let (from_raw, to_raw, label_raw): (Option<Value>, Option<Value>, Option<Value>) = match &e {
            Value::Object(m) => (
                pick_raw(m, &["from", "source", "start", "src"]).cloned(),
                pick_raw(m, &["to", "target", "end", "dst"]).cloned(),
                pick_raw(m, &["label", "text", "note"]).cloned(),
            ),
            Value::Array(a) => (a.first().cloned(), a.get(1).cloned(), a.get(2).cloned()),
            Value::String(s) => {
                let (path, label) = s.split_once(':').unwrap_or((s.as_str(), ""));
                let parts = split_arrows(path);
                if parts.len() < 2 {
                    continue;
                }
                (Some(Value::String(parts[0].clone())), Some(Value::String(parts[1].clone())), Some(Value::String(label.to_string())))
            }
            _ => continue,
        };
        // Endpoint by id, then by label; nodes come from arrows when none were listed.
        let mut resolve = |raw: Option<&Value>| -> Option<String> {
            let s = clamp_str(raw, 48);
            if s.is_empty() {
                return None;
            }
            if ids.contains(&s) {
                return Some(s);
            }
            if let Some(n) = nodes.iter().find(|n| key(&n.label) == key(&s) || key(&n.id) == key(&s)) {
                return Some(n.id.clone());
            }
            let label = clamp_str(raw, MAX_CELL_TEXT);
            (!declared && add(&mut nodes, &mut ids, s.clone(), label, None, None)).then_some(s)
        };
        let Some(from) = resolve(from_raw.as_ref()) else { continue };
        let Some(to) = resolve(to_raw.as_ref()) else { continue };
        let label = clamp_str(label_raw.as_ref(), 48);
        if !seen_edges.insert((from.clone(), to.clone(), label.clone())) {
            continue;
        }
        edges.push(FlowEdge { from, to, label: Some(label).filter(|l| !l.is_empty()) });
    }
    if nodes.is_empty() {
        return None;
    }
    // Nodes listed without any arrows read as a sequence.
    if edges.is_empty() {
        edges = nodes.windows(2).map(|w| FlowEdge { from: w[0].id.clone(), to: w[1].id.clone(), label: None }).collect();
    }
    let direction = flow_direction(pick_raw(o, &["direction", "layout", "orientation"]));
    Some(Block::Flow { direction, nodes, edges })
}

fn compare_kind(v: Option<&Value>) -> Option<String> {
    match key(&clamp_str(v, 16)).as_str() {
        "pro" | "pros" | "plus" | "+" | "good" | "yes" | "advantage" => Some("pro".into()),
        "con" | "cons" | "minus" | "-" | "bad" | "no" | "disadvantage" => Some("con".into()),
        "neutral" => Some("neutral".into()),
        _ => None,
    }
}

/// Reads `+ fast` / `− slow` / `✓` / `✗` prefixes as the item kind.
fn compare_text(raw: String) -> CompareItem {
    let mut chars = raw.chars();
    let kind = match chars.next() {
        Some('+' | '✓' | '✔') => "pro",
        Some('-' | '−' | '–' | '✗' | '✘' | '×') => "con",
        _ => return CompareItem { text: raw, kind: None },
    };
    let rest = chars.as_str().trim_start();
    if rest.is_empty() {
        return CompareItem { text: raw, kind: None };
    }
    CompareItem { text: rest.to_string(), kind: Some(kind.into()) }
}

fn norm_compare(o: &Obj) -> Option<Block> {
    let mut columns = Vec::new();
    for col in pick_arr(o, &["columns", "items", "options", "sides"]) {
        if columns.len() >= MAX_COMPARE_COLS {
            break;
        }
        let Value::Object(c) = &col else { continue };
        let title = pick(c, &["title", "name", "label"], MAX_CELL_TEXT);
        if title.is_empty() {
            continue;
        }
        let mut items: Vec<CompareItem> = Vec::new();
        let mut add = |raw: &Value, forced: Option<&str>| {
            if items.len() >= MAX_COMPARE_ITEMS {
                return;
            }
            if let Value::Object(m) = raw {
                let text = pick(m, &["text", "label", "title", "value"], MAX_SHORT_TEXT);
                if text.is_empty() {
                    return;
                }
                match forced.map(str::to_string).or_else(|| compare_kind(pick_raw(m, &["kind", "type"]))) {
                    Some(kind) => items.push(CompareItem { text, kind: Some(kind) }),
                    None => items.push(compare_text(text)),
                }
            } else {
                let text = clamp_str(Some(raw), MAX_SHORT_TEXT);
                if text.is_empty() {
                    return;
                }
                match forced {
                    Some(kind) => items.push(CompareItem { text, kind: Some(kind.into()) }),
                    None => items.push(compare_text(text)),
                }
            }
        };
        for it in pick_arr(c, &["items", "points", "features"]) {
            add(&it, None);
        }
        for it in pick_arr(c, &["pros", "plus", "advantages"]) {
            add(&it, Some("pro"));
        }
        for it in pick_arr(c, &["cons", "minus", "disadvantages"]) {
            add(&it, Some("con"));
        }
        if items.is_empty() {
            continue;
        }
        let subtitle = opt_pick(c, &["subtitle", "sub", "note", "price"], MAX_CELL_TEXT);
        let accent = to_bool(pick_raw(c, &["accent", "highlight", "recommended", "winner"])).then_some(true);
        columns.push(CompareColumn { title, subtitle, accent, items });
    }
    (!columns.is_empty()).then_some(Block::Compare { columns })
}

fn norm_math(o: &Obj) -> Option<Block> {
    let mut lines = Vec::new();
    for it in pick_arr(o, &["lines", "steps", "items"]) {
        if lines.len() >= MAX_MATH_LINES {
            break;
        }
        if let Value::Object(m) = &it {
            let expr = pick(m, &["expr", "expression", "formula", "math", "latex", "line", "text"], MAX_SHORT_TEXT);
            if expr.is_empty() {
                continue;
            }
            let note = opt_pick(m, &["note", "comment", "explanation", "why", "label"], MAX_SHORT_TEXT).filter(|n| *n != expr);
            lines.push(MathLine { expr, note });
        } else {
            let expr = clamp_str(Some(&it), MAX_SHORT_TEXT);
            if !expr.is_empty() {
                lines.push(MathLine { expr, note: None });
            }
        }
    }
    if lines.is_empty() {
        let expr = pick(o, &["expr", "formula", "expression"], MAX_SHORT_TEXT);
        if !expr.is_empty() {
            lines.push(MathLine { expr, note: None });
        }
    }
    if lines.is_empty() {
        return None;
    }
    Some(Block::Math { lines, result: opt_pick(o, &["result", "answer"], MAX_SHORT_TEXT) })
}

/// A list value joined with `; `, any other value as a string.
fn join_list(v: Option<&Value>, max: usize) -> String {
    match v {
        Some(Value::Array(a)) => {
            let parts: Vec<String> = a.iter().map(|x| clamp_str(Some(x), max)).filter(|s| !s.is_empty()).collect();
            clamp_str(Some(&Value::String(parts.join("; "))), max)
        }
        other => clamp_str(other, max),
    }
}

fn norm_definition(o: &Obj) -> Option<Block> {
    let term = pick(o, &["term", "word", "title", "name"], MAX_CELL_TEXT);
    let meaning = pick(o, &["meaning", "definition", "text", "description", "value"], MAX_TEXT);
    if term.is_empty() || meaning.is_empty() {
        return None;
    }
    let example = Some(join_list(pick_raw(o, &["example", "examples", "usage"]), MAX_SHORT_TEXT)).filter(|s| !s.is_empty());
    Some(Block::Definition { term, meaning, example })
}

const LETTERS_LAT: [char; 6] = ['a', 'b', 'c', 'd', 'e', 'f'];
const LETTERS_CYR: [char; 6] = ['а', 'б', 'в', 'г', 'д', 'е'];

fn norm_quiz(o: &Obj) -> Option<Block> {
    let question = pick(o, &["question", "prompt", "q", "text", "title"], MAX_SHORT_TEXT);
    let mut options: Vec<String> = Vec::new();
    let mut flagged: Option<usize> = None;
    for opt in pick_arr(o, &["options", "choices", "answers", "variants"]) {
        if options.len() >= MAX_QUIZ_OPTIONS {
            break;
        }
        let text = match &opt {
            Value::Object(m) => pick(m, &["text", "label", "title", "value"], MAX_SHORT_TEXT),
            other => clamp_str(Some(other), MAX_SHORT_TEXT),
        };
        if text.is_empty() {
            continue;
        }
        if let Value::Object(m) = &opt {
            if flagged.is_none() && to_bool(pick_raw(m, &["correct", "isCorrect", "right"])) {
                flagged = Some(options.len());
            }
        }
        options.push(text);
    }
    if question.is_empty() || options.len() < 2 {
        return None;
    }

    let last = options.len() - 1;
    let mut correct = flagged;
    let raw = pick_raw(o, &["correct", "correctIndex", "correct_index", "answerIndex", "answer_index", "answer", "right"]);
    if correct.is_none() {
        let (as_text, n): (String, Option<f64>) = match raw {
            Some(Value::Number(n)) => (n.to_string(), n.as_f64()),
            Some(Value::String(s)) => {
                let t = clean_text(s);
                let digits = t.strip_prefix(['-', '+']).unwrap_or(&t);
                let n = (!digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())).then(|| t.parse::<f64>().ok()).flatten();
                (t, n)
            }
            _ => (String::new(), None),
        };
        let lower = as_text.to_lowercase();
        let by_text = options.iter().position(|opt| opt.to_lowercase() == lower);
        let letter: Vec<char> = lower.trim_end_matches([')', '.', ':', ' ', '\t', '\n']).chars().collect();
        let by_letter = (letter.len() == 1)
            .then(|| LETTERS_LAT.iter().position(|c| *c == letter[0]).or_else(|| LETTERS_CYR.iter().position(|c| *c == letter[0])))
            .flatten();
        let is_number = matches!(raw, Some(Value::Number(_)));
        let int = n.filter(|n| n.fract() == 0.0);
        correct = match int {
            Some(n) if n >= 0.0 && (n as usize) <= last && (is_number || by_text.is_none()) => Some(n as usize),
            _ if by_text.is_some() => by_text,
            Some(n) => Some((n.max(0.0) as usize).min(last)),
            None => by_letter.filter(|i| *i <= last),
        };
    }
    // Without a known answer the quiz would mark the wrong option right.
    let correct = correct?;
    let explanation = opt_pick(o, &["explanation", "explain", "why", "rationale", "note"], MAX_TEXT);
    Some(Block::Quiz { question, options, correct, explanation })
}

fn callout_tone(v: &str) -> &'static str {
    match key(v).as_str() {
        "tip" | "hint" | "success" | "good" | "ok" | "done" => "tip",
        "warning" | "warn" | "caution" | "danger" | "alert" | "important" | "error" => "warning",
        _ => "idea",
    }
}

fn norm_callout(o: &Obj, alias: &str) -> Option<Block> {
    let text = pick(o, &["text", "body", "content", "message", "note", "value"], MAX_TEXT);
    if text.is_empty() {
        return None;
    }
    let tone_raw = pick(o, &["tone", "variant", "level", "style"], 16);
    let tone = callout_tone(if tone_raw.is_empty() { alias } else { &tone_raw }).to_string();
    Some(Block::Callout { tone, title: opt_pick(o, &["title", "heading", "label"], MAX_CELL_TEXT), text })
}

fn norm_kv(o: &Obj) -> Option<Block> {
    let mut items: Vec<KvItem> = Vec::new();
    let mut push = |k: Option<&Value>, v: Option<&Value>| {
        if items.len() >= MAX_KV_ITEMS {
            return;
        }
        let key_text = clamp_str(k, MAX_CELL_TEXT);
        let value = join_list(v, MAX_SHORT_TEXT);
        if !key_text.is_empty() && !value.is_empty() {
            items.push(KvItem { key: key_text, value });
        }
    };
    let src = pick_arr(o, &["items", "pairs", "facts", "rows", "data"]);
    for it in &src {
        match it {
            Value::Array(a) => push(a.first(), a.get(1)),
            Value::Object(m) => {
                if let Some(k) = pick_raw(m, &["key", "label", "name", "k", "title"]) {
                    push(Some(k), pick_raw(m, &["value", "val", "v", "text"]));
                } else if m.len() == 1 {
                    let (k, v) = m.iter().next().expect("one entry");
                    push(Some(&Value::String(k.clone())), Some(v));
                }
            }
            _ => {}
        }
    }
    if src.is_empty() {
        for (k, v) in map_entries(pick_raw(o, &["items", "facts", "data", "pairs"])) {
            push(Some(&Value::String(k)), Some(&v));
        }
    }
    (!items.is_empty()).then_some(Block::Kv { items })
}

fn norm_block(raw: &Value) -> Option<Block> {
    let o = match raw {
        Value::String(s) => return norm_text(&Map::from_iter([("text".to_string(), Value::String(s.clone()))])),
        Value::Object(o) => o,
        _ => return None,
    };
    let alias = key(&clamp_str(pick_raw(o, &["kind", "type", "block"]), 32));
    // "chart"/"graph" and anything unknown: look at the data instead.
    match kind_alias(&alias).or_else(|| infer_kind(o))? {
        "text" => norm_text(o),
        "steps" => norm_steps(o),
        "table" => norm_table(o),
        "bars" => norm_bars(o),
        "line" => norm_line(o),
        "timeline" => norm_timeline(o),
        "flow" => norm_flow(o),
        "compare" => norm_compare(o),
        "math" => norm_math(o),
        "definition" => norm_definition(o),
        "quiz" => norm_quiz(o),
        "callout" => norm_callout(o, &alias),
        "kv" => norm_kv(o),
        _ => None,
    }
}

fn json_chars(spec: &CardSpec) -> usize {
    serde_json::to_string(spec).map(|s| s.chars().count()).unwrap_or(usize::MAX)
}

/// Validates and clamps raw tool arguments into a clean [`CardSpec`].
pub fn normalize_card(raw: &Value) -> AppResult<CardSpec> {
    let parsed;
    let raw = match raw {
        Value::String(s) => {
            parsed = serde_json::from_str::<Value>(s).map_err(|_| AppError::invalid("Карточка должна быть JSON-объектом"))?;
            &parsed
        }
        other => other,
    };
    let obj = raw.as_object().ok_or_else(|| AppError::invalid("Аргументы карточки должны быть объектом"))?;
    let mut raw_blocks = pick_arr(obj, &["blocks", "sections", "content"]);
    if raw_blocks.is_empty() && pick_raw(obj, &["kind", "type"]).is_some() {
        raw_blocks = vec![raw.clone()];
    }
    let mut blocks = Vec::new();
    for b in raw_blocks.iter().take(MAX_RAW_BLOCKS) {
        if blocks.len() >= MAX_BLOCKS {
            break;
        }
        if let Some(nb) = norm_block(b) {
            blocks.push(nb);
        }
    }
    if blocks.is_empty() {
        return Err(AppError::invalid(
            "В карточке нет ни одного понятного блока: нужен массив blocks вида [{\"kind\":\"steps\",\"items\":[{\"title\":\"…\"}]}]",
        ));
    }
    let title = Some(pick(obj, &["title", "heading", "name"], MAX_TITLE)).filter(|t| !t.is_empty()).unwrap_or_else(|| FALLBACK_TITLE.into());
    let subtitle = opt_pick(obj, &["subtitle", "sub", "description"], MAX_SUBTITLE);
    let mut spec = CardSpec { title, subtitle, blocks };
    while spec.blocks.len() > 1 && json_chars(&spec) > MAX_CARD_JSON {
        spec.blocks.pop();
    }
    Ok(spec)
}

/// The card for raw `explain_card` arguments as the frontend renders it, if valid.
/// Used for the live chat row and to rebuild cards from saved history.
pub fn card_json(arguments: &str) -> Option<Value> {
    let raw: Value = serde_json::from_str(arguments).ok()?;
    serde_json::to_value(normalize_card(&raw).ok()?).ok()
}

/// The overlay draw item: `{ "kind": "card", "id": "...", "anchor": {x,y,w,h}?, "spec": CardSpec }`.
pub fn overlay_item(id: &str, spec: &CardSpec, anchor: Option<AnchorRect>) -> Value {
    let mut map = Map::new();
    map.insert("kind".into(), json!("card"));
    map.insert("id".into(), json!(id));
    if let Some(a) = anchor {
        map.insert("anchor".into(), json!({ "x": a.x, "y": a.y, "w": a.w, "h": a.h }));
    }
    map.insert("spec".into(), serde_json::to_value(spec).unwrap_or_else(|_| json!({})));
    Value::Object(map)
}

/// Short Russian summary for the tool result and the chat row.
pub fn card_summary(spec: &CardSpec) -> String {
    let n = spec.blocks.len();
    let word = match (n % 10, n % 100) {
        (1, k) if k != 11 => "блок",
        (2..=4, k) if !(12..=14).contains(&k) => "блока",
        _ => "блоков",
    };
    format!("Карточка «{}» на экране ({n} {word})", spec.title)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(block: Value) -> Block {
        normalize_card(&json!({ "title": "T", "blocks": [block] })).expect("valid card").blocks.remove(0)
    }

    #[test]
    fn rejects_cards_without_usable_blocks() {
        assert!(normalize_card(&json!({})).is_err());
        assert!(normalize_card(&json!(42)).is_err());
        assert!(normalize_card(&json!({ "title": "Заголовок", "blocks": [] })).is_err());
        assert!(normalize_card(&json!({ "title": "Заголовок", "blocks": [{ "kind": "unknown" }, 42] })).is_err());
    }

    #[test]
    fn missing_title_falls_back_and_stringified_input_is_read() {
        let spec = normalize_card(&json!({ "title": "   ", "blocks": [{ "kind": "text", "text": "x" }] })).unwrap();
        assert_eq!(spec.title, FALLBACK_TITLE);
        let spec = normalize_card(&json!(r#"{"title":"S","blocks":"[\"просто текст\"]"}"#)).unwrap();
        assert_eq!(spec.blocks, vec![Block::Text { text: "просто текст".into() }]);
    }

    #[test]
    fn reads_numbers_the_way_people_write_them() {
        let n = |s: &str| parse_number(Some(&json!(s)));
        assert_eq!(n("42,5"), Some(42.5));
        assert_eq!(n("1 200 ₽"), Some(1200.0));
        assert_eq!(n("$1,234.5"), Some(1234.5));
        assert_eq!(n("1.234,5"), Some(1234.5));
        assert_eq!(n("1,500"), Some(1500.0));
        assert_eq!(n("0,125"), Some(0.125));
        assert_eq!(n("−7 %"), Some(-7.0));
        assert_eq!(n("около 15 мс"), Some(15.0));
        assert_eq!(n("abc"), None);
        assert_eq!(parse_number(Some(&json!(true))), None);
        assert_eq!(parse_number(Some(&json!(3))), Some(3.0));
    }

    #[test]
    fn normalizes_all_block_kinds_and_coerces_strings() {
        let raw = json!({
            "title": "  Полный обзор  ",
            "subtitle": "Проверка всех блоков",
            "blocks": [
                { "kind": "text", "text": "**Жирный** и `код`" },
                { "kind": "steps", "items": ["Шаг один", { "title": "Шаг два", "note": "Деталь" }] },
                { "kind": "table", "headers": ["Имя", "Балл"], "rows": [["А", 10, "лишнее"], ["Б"]], "highlight_col": "1" },
                { "kind": "bars", "unit": "%", "items": [{ "label": "Q1", "value": "42,5", "highlight": true }, { "label": "bad", "value": "NaN" }] },
                { "kind": "line", "x_labels": ["Пн", "Вт", "Ср"], "values": ["10", 20, "30.5"] },
                { "kind": "timeline", "items": [{ "date": 1991, "label": "Python 0.9", "note": "Первый релиз" }] },
                { "kind": "flow", "direction": "LR", "nodes": [{ "id": "a", "label": "Старт", "tone": "accent" }, { "id": "b", "label": "Финиш" }, { "id": "a", "label": "Дубль" }], "edges": [{ "from": "a", "to": "b", "label": "ok" }, { "from": "a", "to": "ghost" }] },
                { "kind": "quiz", "question": "2 + 2?", "options": ["3", "4", "5"], "correct": "99", "explanation": "Четыре" }
            ]
        });
        let spec = normalize_card(&raw).expect("valid card");
        assert_eq!(spec.title, "Полный обзор");
        assert_eq!(spec.blocks.len(), 8);
        match &spec.blocks[2] {
            Block::Table { headers, rows, highlight_col } => {
                assert_eq!(headers.len(), 2);
                assert_eq!(rows[0], vec!["А", "10"]);
                assert_eq!(rows[1], vec!["Б", ""]);
                assert_eq!(*highlight_col, Some(1));
            }
            _ => panic!("expected table"),
        }
        match &spec.blocks[3] {
            Block::Bars { items, .. } => {
                assert_eq!(items.len(), 1);
                assert!((items[0].value - 42.5).abs() < 1e-6);
                assert_eq!(items[0].highlight, Some(true));
            }
            _ => panic!("expected bars"),
        }
        match &spec.blocks[4] {
            Block::Line { x_labels, series, .. } => {
                assert_eq!(x_labels, &vec!["Пн", "Вт", "Ср"]);
                assert_eq!(series[0].values, vec![10.0, 20.0, 30.5]);
            }
            _ => panic!("expected line"),
        }
        match &spec.blocks[6] {
            Block::Flow { direction, nodes, edges } => {
                assert_eq!(direction.as_deref(), Some("lr"));
                assert_eq!(nodes.len(), 2);
                assert_eq!(edges.len(), 1);
            }
            _ => panic!("expected flow"),
        }
        assert!(matches!(&spec.blocks[7], Block::Quiz { correct: 2, .. }));
    }

    #[test]
    fn accepts_kind_aliases_and_shapes_without_a_kind() {
        let spec = normalize_card(&json!({
            "title": "Небрежная карточка",
            "blocks": [
                "Просто строка",
                { "type": "bar_chart", "items": [["Январь", "10"], ["Февраль", 12]] },
                { "type": "warning", "text": "Осторожно" },
                { "type": "key-value", "items": [{ "Порт": 443 }, ["Протокол", "HTTPS"]] },
                { "headers": ["", "A"], "rows": [{ "": "x", "a": 1 }] },
                { "nodes": ["Запрос", "Ответ"] },
                { "question": "2 + 2?", "options": ["3", "4"], "answer": "4" },
                { "type": "chart", "series": [{ "name": "S", "data": [{ "x": "Пн", "y": 1 }, { "x": "Вт", "y": 2 }] }] }
            ]
        }))
        .unwrap();
        let kinds: Vec<&str> = spec.blocks.iter().map(Block::kind).collect();
        assert_eq!(kinds, vec!["text", "bars", "callout", "kv", "table", "flow", "quiz", "line"]);
        assert!(matches!(&spec.blocks[1], Block::Bars { items, .. } if items.len() == 2 && items[0].value == 10.0));
        assert!(matches!(&spec.blocks[2], Block::Callout { tone, .. } if tone == "warning"));
        assert!(matches!(&spec.blocks[3], Block::Kv { items } if items[0].value == "443" && items[1].key == "Протокол"));
        assert!(matches!(&spec.blocks[4], Block::Table { rows, .. } if rows[0] == vec!["x", "1"]));
        assert!(matches!(&spec.blocks[5], Block::Flow { edges, .. } if edges.len() == 1 && edges[0].from == "Запрос"));
        assert!(matches!(&spec.blocks[6], Block::Quiz { correct: 1, .. }));
        assert!(matches!(&spec.blocks[7], Block::Line { x_labels, .. } if x_labels == &vec!["Пн", "Вт"]));
    }

    #[test]
    fn resolves_quiz_answers_and_drops_unknown_ones() {
        let quiz = |extra: Value| {
            let mut b = json!({ "kind": "quiz", "question": "2 + 2?", "options": ["3", "4", "5"] });
            b.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            normalize_card(&json!({ "title": "Q", "blocks": [b] })).ok().map(|s| s.blocks[0].clone())
        };
        let correct = |b: Option<Block>| match b {
            Some(Block::Quiz { correct, .. }) => Some(correct),
            _ => None,
        };
        assert_eq!(correct(quiz(json!({ "correct": 1 }))), Some(1));
        assert_eq!(correct(quiz(json!({ "correct": "4" }))), Some(1));
        assert_eq!(correct(quiz(json!({ "answer": "Б" }))), Some(1));
        assert_eq!(correct(quiz(json!({ "answer": "b)" }))), Some(1));
        assert_eq!(correct(quiz(json!({ "options": ["3", { "text": "4", "correct": true }, "5"] }))), Some(1));
        assert_eq!(correct(quiz(json!({ "answer": "не знаю" }))), None);
        assert_eq!(correct(quiz(json!({}))), None);
    }

    #[test]
    fn reads_flow_edges_written_as_arrows_tuples_and_labels() {
        let b = one(json!({
            "kind": "flow",
            "direction": "horizontal",
            "nodes": [{ "id": "a", "label": "Клиент", "tone": "highlight" }, { "id": "b", "label": "Сервер" }, "База"],
            "edges": ["Клиент -> b: HTTP", ["b", "База"], { "source": "a", "target": "nowhere" }]
        }));
        let Block::Flow { direction, nodes, edges } = b else { panic!("expected flow") };
        assert_eq!(direction.as_deref(), Some("lr"));
        assert_eq!(nodes[0].tone.as_deref(), Some("accent"));
        assert_eq!(nodes[2].id, "База");
        assert_eq!(
            edges,
            vec![
                FlowEdge { from: "a".into(), to: "b".into(), label: Some("HTTP".into()) },
                FlowEdge { from: "b".into(), to: "База".into(), label: None },
            ]
        );
        let Block::Flow { nodes, .. } = one(json!({ "kind": "flow", "edges": ["Идея → Прототип", "Прототип ==> Релиз"] })) else {
            panic!("expected flow")
        };
        let ids: Vec<&str> = nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, vec!["Идея", "Прототип", "Релиз"]);
    }

    #[test]
    fn reads_compare_prefixes_lists_and_strips_control_characters() {
        let spec = normalize_card(&json!({
            "title": "Сравнение\u{7}",
            "blocks": [
                { "kind": "compare", "columns": [{ "title": "A", "items": ["+ Быстро", "− Дорого", "Просто"], "recommended": "yes" }] },
                { "kind": "definition", "term": "Кэш", "meaning": "Быстрая память", "examples": ["L1", "L2"] },
                { "kind": "math", "lines": [{ "expr": "2x = 8", "note": "делим на 2" }], "result": "x = 4" }
            ]
        }))
        .unwrap();
        assert_eq!(spec.title, "Сравнение");
        let Block::Compare { columns } = &spec.blocks[0] else { panic!("expected compare") };
        assert_eq!(columns[0].accent, Some(true));
        let kinds: Vec<Option<&str>> = columns[0].items.iter().map(|i| i.kind.as_deref()).collect();
        assert_eq!(kinds, vec![Some("pro"), Some("con"), None]);
        assert_eq!(columns[0].items[0].text, "Быстро");
        assert!(matches!(&spec.blocks[1], Block::Definition { example: Some(e), .. } if e == "L1; L2"));
        assert!(matches!(&spec.blocks[2], Block::Math { result: Some(r), .. } if r == "x = 4"));
    }

    #[test]
    fn clamps_oversized_cards_rows_and_unicode_strings() {
        let many_blocks: Vec<Value> = (0..15).map(|i| json!({ "kind": "text", "text": format!("Блок {i}") })).collect();
        let spec = normalize_card(&json!({ "title": "Ж".repeat(200), "blocks": many_blocks })).unwrap();
        assert_eq!(spec.title.chars().count(), MAX_TITLE);
        assert!(spec.title.ends_with('…'));
        assert_eq!(spec.blocks.len(), MAX_BLOCKS);

        let many_rows: Vec<Value> = (0..25).map(|i| json!([format!("r{i}"), i])).collect();
        let Block::Table { rows, .. } = one(json!({ "kind": "table", "headers": ["A", "B"], "rows": many_rows })) else {
            panic!("expected table")
        };
        assert_eq!(rows.len(), MAX_TABLE_ROWS);

        // Eight full tables would be a wall of data: trailing blocks go first.
        let wide_row: Vec<String> = (0..6).map(|_| "Я".repeat(200)).collect();
        let table = json!({ "kind": "table", "headers": ["1", "2", "3", "4", "5", "6"], "rows": vec![wide_row; 12] });
        let spec = normalize_card(&json!({ "title": "Много", "blocks": vec![table; 8] })).unwrap();
        assert!(spec.blocks.len() < 8 && !spec.blocks.is_empty());
        assert!(json_chars(&spec) <= MAX_CARD_JSON);
    }

    #[test]
    fn normalizing_twice_changes_nothing() {
        let once = normalize_card(&json!({
            "title": "Всё сразу",
            "subtitle": "s",
            "blocks": [
                { "kind": "table", "headers": ["", "B"], "rows": [["a", 1]], "highlightCol": 1 },
                { "kind": "flow", "nodes": [{ "id": "x", "label": "X", "tone": "muted", "note": "n" }, "Y"], "edges": [] },
                { "kind": "callout", "tone": "tip", "title": "t", "text": "c" },
                { "kind": "quiz", "question": "?", "options": ["a", "b"], "correct": 1, "explanation": "e" },
                { "kind": "line", "values": [1, 2, 3] },
                { "kind": "compare", "columns": [{ "title": "A", "pros": ["p"], "cons": ["c"] }] },
                { "kind": "math", "lines": ["x = 1"], "result": "1" },
                { "kind": "kv", "items": [{ "key": "k", "value": "v" }] }
            ]
        }))
        .unwrap();
        let twice = normalize_card(&serde_json::to_value(&once).unwrap()).unwrap();
        assert_eq!(once, twice);
    }

    #[test]
    fn card_json_and_overlay_item_match_the_contract() {
        let args = r#"{"title":"DNS","blocks":[{"kind":"text","text":"Кратко"}]}"#;
        let card = card_json(args).expect("card");
        assert_eq!(card["blocks"][0]["kind"], "text");
        assert!(card_json("not json").is_none());
        assert!(card_json(r#"{"title":"x","blocks":[]}"#).is_none());

        let spec = normalize_card(&serde_json::from_str(args).unwrap()).unwrap();
        let item = overlay_item("card-1", &spec, Some(AnchorRect { x: 10, y: 20, w: 300, h: 40 }));
        assert_eq!(item["kind"], "card");
        assert_eq!(item["id"], "card-1");
        assert_eq!(item["anchor"], json!({ "x": 10, "y": 20, "w": 300, "h": 40 }));
        assert_eq!(item["spec"]["title"], "DNS");
        assert!(overlay_item("card-2", &spec, None).get("anchor").is_none());
        assert_eq!(card_summary(&spec), "Карточка «DNS» на экране (1 блок)");
    }
}
