//! Explanation card data model, tolerant validation and clamping.
//! Mirrors `src/overlay/cards/types.ts` and `src/overlay/cards/normalize.ts`.

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

/// Physical screen-pixel rectangle used by the overlay to place a card beside an element.
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

fn clamp_str(v: Option<&Value>, max: usize) -> String {
    let raw = match v {
        Some(Value::String(s)) => s.trim().to_string(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => if *b { "Да" } else { "Нет" }.to_string(),
        _ => return String::new(),
    };
    if raw.is_empty() {
        return String::new();
    }
    let count = raw.chars().count();
    if count <= max {
        raw
    } else {
        let head: String = raw.chars().take(max.saturating_sub(1).max(1)).collect();
        format!("{}…", head.trim_end())
    }
}

fn opt_str(v: Option<&Value>, max: usize) -> Option<String> {
    let s = clamp_str(v, max);
    if s.is_empty() { None } else { Some(s) }
}

fn first<'a>(obj: &'a Map<String, Value>, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|k| obj.get(*k))
}

fn to_num(v: Option<&Value>) -> Option<f64> {
    match v {
        Some(Value::Number(n)) => n.as_f64().filter(|f| f.is_finite()),
        Some(Value::String(s)) => {
            let cleaned: String = s.chars().filter(|c| !c.is_whitespace()).collect();
            let cleaned = cleaned.replace(',', ".");
            cleaned.parse::<f64>().ok().filter(|f| f.is_finite())
        }
        _ => None,
    }
}

fn to_bool(v: Option<&Value>) -> bool {
    match v {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => s.trim().eq_ignore_ascii_case("true"),
        _ => false,
    }
}

fn norm_text(obj: &Map<String, Value>) -> Option<Block> {
    let text = clamp_str(first(obj, &["text", "content", "body"]), MAX_TEXT);
    if text.is_empty() { None } else { Some(Block::Text { text }) }
}

fn norm_steps(obj: &Map<String, Value>) -> Option<Block> {
    let arr = first(obj, &["items", "steps"]).and_then(Value::as_array)?;
    let mut items = Vec::new();
    for it in arr {
        if items.len() >= MAX_STEPS {
            break;
        }
        match it {
            Value::String(_) => {
                let title = clamp_str(Some(it), MAX_SHORT_TEXT);
                if !title.is_empty() {
                    items.push(StepItem { title, note: None });
                }
            }
            Value::Object(m) => {
                let title = clamp_str(first(m, &["title", "label", "text"]), MAX_SHORT_TEXT);
                if title.is_empty() {
                    continue;
                }
                let note = opt_str(first(m, &["note", "detail", "description", "sub"]), MAX_SHORT_TEXT);
                items.push(StepItem { title, note });
            }
            _ => {}
        }
    }
    if items.is_empty() { None } else { Some(Block::Steps { items }) }
}

fn norm_table(obj: &Map<String, Value>) -> Option<Block> {
    let raw_headers = first(obj, &["headers", "columns"]).and_then(Value::as_array)?;
    let headers: Vec<String> = raw_headers
        .iter()
        .take(MAX_TABLE_COLS)
        .map(|h| clamp_str(Some(h), MAX_CELL_TEXT))
        .filter(|s| !s.is_empty())
        .collect();
    if headers.is_empty() {
        return None;
    }

    let raw_rows = obj.get("rows").and_then(Value::as_array)?;
    let mut rows = Vec::new();
    for r in raw_rows {
        if rows.len() >= MAX_TABLE_ROWS {
            break;
        }
        let Some(arr) = r.as_array() else { continue };
        let cells: Vec<String> = (0..headers.len())
            .map(|c| clamp_str(arr.get(c), MAX_CELL_TEXT))
            .collect();
        if cells.iter().any(|c| !c.is_empty()) {
            rows.push(cells);
        }
    }
    if rows.is_empty() {
        return None;
    }

    let highlight_col = to_num(first(obj, &["highlightCol", "highlight_col"])).and_then(|n| {
        if n.fract() == 0.0 && n >= 0.0 && (n as usize) < headers.len() {
            Some(n as usize)
        } else {
            None
        }
    });

    Some(Block::Table { headers, rows, highlight_col })
}

fn norm_bars(obj: &Map<String, Value>) -> Option<Block> {
    let arr = first(obj, &["items", "bars"]).and_then(Value::as_array)?;
    let mut items = Vec::new();
    for it in arr {
        if items.len() >= MAX_BARS {
            break;
        }
        let Some(m) = it.as_object() else { continue };
        let label = clamp_str(first(m, &["label", "name", "key"]), MAX_CELL_TEXT);
        let Some(value) = to_num(first(m, &["value", "val", "y"])) else { continue };
        if label.is_empty() {
            continue;
        }
        let highlight = to_bool(m.get("highlight")).then_some(true);
        items.push(BarItem { label, value, highlight });
    }
    if items.is_empty() {
        return None;
    }
    Some(Block::Bars {
        title: opt_str(obj.get("title"), MAX_CELL_TEXT),
        unit: opt_str(first(obj, &["unit", "units"]), 24),
        items,
    })
}

fn norm_line(obj: &Map<String, Value>) -> Option<Block> {
    let title = opt_str(obj.get("title"), MAX_CELL_TEXT);
    let unit = opt_str(first(obj, &["unit", "units"]), 24);
    let mut x_labels: Vec<String> = Vec::new();
    let mut series: Vec<LineSeries> = Vec::new();

    if let Some(pts) = obj.get("points").and_then(Value::as_array).filter(|_| obj.get("series").is_none()) {
        let mut values = Vec::new();
        for pt in pts {
            if values.len() >= MAX_LINE_POINTS {
                break;
            }
            let Some(m) = pt.as_object() else { continue };
            let Some(y) = to_num(first(m, &["y", "value"])) else { continue };
            let lbl = clamp_str(first(m, &["x", "label"]), 24);
            x_labels.push(if lbl.is_empty() { (values.len() + 1).to_string() } else { lbl });
            values.push(y);
        }
        if values.len() >= 2 {
            series.push(LineSeries { name: None, values });
        }
    } else {
        if let Some(lbls) = first(obj, &["xLabels", "x_labels", "labels"]).and_then(Value::as_array) {
            for (idx, l) in lbls.iter().take(MAX_LINE_POINTS).enumerate() {
                let s = clamp_str(Some(l), 24);
                x_labels.push(if s.is_empty() { (idx + 1).to_string() } else { s });
            }
        }
        if let Some(raw_series) = obj.get("series").and_then(Value::as_array) {
            for s in raw_series {
                if series.len() >= MAX_LINE_SERIES {
                    break;
                }
                let Some(m) = s.as_object() else { continue };
                let Some(vals) = first(m, &["values", "points"]).and_then(Value::as_array) else { continue };
                let values: Vec<f64> = vals.iter().take(MAX_LINE_POINTS).filter_map(|v| to_num(Some(v))).collect();
                if values.len() < 2 {
                    continue;
                }
                series.push(LineSeries { name: opt_str(first(m, &["name", "label"]), 48), values });
            }
        } else if let Some(vals) = obj.get("values").and_then(Value::as_array) {
            let values: Vec<f64> = vals.iter().take(MAX_LINE_POINTS).filter_map(|v| to_num(Some(v))).collect();
            if values.len() >= 2 {
                series.push(LineSeries { name: None, values });
            }
        }
    }

    if series.is_empty() {
        return None;
    }
    let max_len = series.iter().map(|s| s.values.len()).max().unwrap_or(0).min(MAX_LINE_POINTS);
    while x_labels.len() < max_len {
        x_labels.push((x_labels.len() + 1).to_string());
    }
    x_labels.truncate(max_len);

    Some(Block::Line { title, unit, x_labels, series })
}

fn norm_timeline(obj: &Map<String, Value>) -> Option<Block> {
    let arr = first(obj, &["items", "events"]).and_then(Value::as_array)?;
    let mut items = Vec::new();
    for it in arr {
        if items.len() >= MAX_TIMELINE {
            break;
        }
        let Some(m) = it.as_object() else { continue };
        let date = clamp_str(first(m, &["date", "time", "year"]), 40);
        let label = clamp_str(first(m, &["label", "title", "event"]), MAX_SHORT_TEXT);
        if date.is_empty() || label.is_empty() {
            continue;
        }
        let note = opt_str(first(m, &["note", "detail", "description"]), MAX_SHORT_TEXT);
        items.push(TimelineItem { date, label, note });
    }
    if items.is_empty() { None } else { Some(Block::Timeline { items }) }
}

fn norm_flow(obj: &Map<String, Value>) -> Option<Block> {
    let raw_nodes = obj.get("nodes").and_then(Value::as_array)?;
    let mut nodes = Vec::new();
    let mut seen = HashSet::new();
    for n in raw_nodes {
        if nodes.len() >= MAX_FLOW_NODES {
            break;
        }
        let Some(m) = n.as_object() else { continue };
        let id = clamp_str(m.get("id"), 48);
        let label = clamp_str(first(m, &["label", "title", "text"]).or_else(|| m.get("id")), MAX_CELL_TEXT);
        if id.is_empty() || label.is_empty() || !seen.insert(id.clone()) {
            continue;
        }
        let note = opt_str(first(m, &["note", "sub"]), MAX_CELL_TEXT);
        let tone = match clamp_str(m.get("tone"), 16).as_str() {
            "accent" => Some("accent".into()),
            "muted" => Some("muted".into()),
            _ => None,
        };
        nodes.push(FlowNode { id, label, note, tone });
    }
    if nodes.is_empty() {
        return None;
    }

    let mut edges = Vec::new();
    let mut edge_keys = HashSet::new();
    if let Some(raw_edges) = obj.get("edges").and_then(Value::as_array) {
        for e in raw_edges {
            if edges.len() >= MAX_FLOW_EDGES {
                break;
            }
            let Some(m) = e.as_object() else { continue };
            let from = clamp_str(first(m, &["from", "source"]), 48);
            let to = clamp_str(first(m, &["to", "target"]), 48);
            if from.is_empty() || to.is_empty() || !seen.contains(&from) || !seen.contains(&to) {
                continue;
            }
            let label = opt_str(m.get("label"), 48);
            let key = format!("{from}->{to}:{}", label.as_deref().unwrap_or(""));
            if !edge_keys.insert(key) {
                continue;
            }
            edges.push(FlowEdge { from, to, label });
        }
    }

    let direction = match clamp_str(obj.get("direction"), 8).to_lowercase().as_str() {
        "lr" => Some("lr".into()),
        "td" => Some("td".into()),
        _ => None,
    };

    Some(Block::Flow { direction, nodes, edges })
}

fn norm_compare_kind(v: Option<&Value>) -> Option<String> {
    match clamp_str(v, 16).to_lowercase().as_str() {
        "pro" | "plus" | "+" => Some("pro".into()),
        "con" | "minus" | "-" => Some("con".into()),
        "neutral" => Some("neutral".into()),
        _ => None,
    }
}

fn norm_compare(obj: &Map<String, Value>) -> Option<Block> {
    let raw_cols = first(obj, &["columns", "items"]).and_then(Value::as_array)?;
    let mut columns = Vec::new();
    for col in raw_cols {
        if columns.len() >= MAX_COMPARE_COLS {
            break;
        }
        let Some(m) = col.as_object() else { continue };
        let title = clamp_str(first(m, &["title", "name"]), MAX_CELL_TEXT);
        if title.is_empty() {
            continue;
        }
        let subtitle = opt_str(first(m, &["subtitle", "sub"]), MAX_CELL_TEXT);
        let accent = to_bool(m.get("accent")).then_some(true);
        let mut items = Vec::new();

        if let Some(arr) = m.get("items").and_then(Value::as_array) {
            for it in arr {
                if items.len() >= MAX_COMPARE_ITEMS {
                    break;
                }
                match it {
                    Value::String(_) => {
                        let text = clamp_str(Some(it), MAX_SHORT_TEXT);
                        if !text.is_empty() {
                            items.push(CompareItem { text, kind: None });
                        }
                    }
                    Value::Object(im) => {
                        let text = clamp_str(first(im, &["text", "label"]), MAX_SHORT_TEXT);
                        if text.is_empty() {
                            continue;
                        }
                        let kind = norm_compare_kind(first(im, &["kind", "type"]));
                        items.push(CompareItem { text, kind });
                    }
                    _ => {}
                }
            }
        } else {
            if let Some(pros) = m.get("pros").and_then(Value::as_array) {
                for p in pros {
                    if items.len() >= MAX_COMPARE_ITEMS {
                        break;
                    }
                    let text = clamp_str(Some(p), MAX_SHORT_TEXT);
                    if !text.is_empty() {
                        items.push(CompareItem { text, kind: Some("pro".into()) });
                    }
                }
            }
            if let Some(cons) = m.get("cons").and_then(Value::as_array) {
                for c in cons {
                    if items.len() >= MAX_COMPARE_ITEMS {
                        break;
                    }
                    let text = clamp_str(Some(c), MAX_SHORT_TEXT);
                    if !text.is_empty() {
                        items.push(CompareItem { text, kind: Some("con".into()) });
                    }
                }
            }
        }

        if items.is_empty() {
            continue;
        }
        columns.push(CompareColumn { title, subtitle, accent, items });
    }
    if columns.is_empty() { None } else { Some(Block::Compare { columns }) }
}

fn norm_math(obj: &Map<String, Value>) -> Option<Block> {
    let arr = first(obj, &["lines", "steps"]).and_then(Value::as_array)?;
    let mut lines = Vec::new();
    for it in arr {
        if lines.len() >= MAX_MATH_LINES {
            break;
        }
        match it {
            Value::String(_) => {
                let expr = clamp_str(Some(it), MAX_SHORT_TEXT);
                if !expr.is_empty() {
                    lines.push(MathLine { expr, note: None });
                }
            }
            Value::Object(m) => {
                let expr = clamp_str(first(m, &["expr", "expression", "formula", "text"]), MAX_SHORT_TEXT);
                if expr.is_empty() {
                    continue;
                }
                let note = opt_str(first(m, &["note", "label", "comment"]), MAX_SHORT_TEXT);
                lines.push(MathLine { expr, note });
            }
            _ => {}
        }
    }
    if lines.is_empty() {
        return None;
    }
    let result = opt_str(first(obj, &["result", "answer"]), MAX_SHORT_TEXT);
    Some(Block::Math { lines, result })
}

fn norm_definition(obj: &Map<String, Value>) -> Option<Block> {
    let term = clamp_str(first(obj, &["term", "title", "word"]), MAX_CELL_TEXT);
    let meaning = clamp_str(first(obj, &["meaning", "definition", "text"]), MAX_TEXT);
    if term.is_empty() || meaning.is_empty() {
        return None;
    }
    let example = opt_str(obj.get("example"), MAX_SHORT_TEXT);
    Some(Block::Definition { term, meaning, example })
}

fn norm_quiz(obj: &Map<String, Value>) -> Option<Block> {
    let question = clamp_str(first(obj, &["question", "prompt"]), MAX_SHORT_TEXT);
    let raw_opts = first(obj, &["options", "choices"]).and_then(Value::as_array)?;
    let options: Vec<String> = raw_opts
        .iter()
        .take(MAX_QUIZ_OPTIONS)
        .map(|o| clamp_str(Some(o), MAX_SHORT_TEXT))
        .filter(|s| !s.is_empty())
        .collect();
    if question.is_empty() || options.len() < 2 {
        return None;
    }
    let correct = to_num(first(obj, &["correct", "correctIndex", "correct_index", "answer"]))
        .filter(|n| n.fract() == 0.0)
        .map(|n| (n.max(0.0) as usize).min(options.len() - 1))
        .unwrap_or(0);
    let explanation = opt_str(first(obj, &["explanation", "note"]), MAX_TEXT);
    Some(Block::Quiz { question, options, correct, explanation })
}

fn norm_callout(obj: &Map<String, Value>) -> Option<Block> {
    let text = clamp_str(first(obj, &["text", "body", "content"]), MAX_TEXT);
    if text.is_empty() {
        return None;
    }
    let tone = match clamp_str(first(obj, &["tone", "variant"]), 16).to_lowercase().as_str() {
        "tip" => "tip",
        "warning" => "warning",
        _ => "idea",
    }
    .to_string();
    let title = opt_str(obj.get("title"), MAX_CELL_TEXT);
    Some(Block::Callout { tone, title, text })
}

fn norm_kv(obj: &Map<String, Value>) -> Option<Block> {
    let arr = first(obj, &["items", "pairs"]).and_then(Value::as_array)?;
    let mut items = Vec::new();
    for it in arr {
        if items.len() >= MAX_KV_ITEMS {
            break;
        }
        let Some(m) = it.as_object() else { continue };
        let key = clamp_str(first(m, &["key", "label", "name"]), MAX_CELL_TEXT);
        let value = clamp_str(first(m, &["value", "val"]), MAX_SHORT_TEXT);
        if key.is_empty() || value.is_empty() {
            continue;
        }
        items.push(KvItem { key, value });
    }
    if items.is_empty() { None } else { Some(Block::Kv { items }) }
}

fn norm_block(raw: &Value) -> Option<Block> {
    let obj = raw.as_object()?;
    match clamp_str(first(obj, &["kind", "type"]), 24).to_lowercase().as_str() {
        "text" => norm_text(obj),
        "steps" => norm_steps(obj),
        "table" => norm_table(obj),
        "bars" => norm_bars(obj),
        "line" => norm_line(obj),
        "timeline" => norm_timeline(obj),
        "flow" => norm_flow(obj),
        "compare" => norm_compare(obj),
        "math" => norm_math(obj),
        "definition" => norm_definition(obj),
        "quiz" => norm_quiz(obj),
        "callout" => norm_callout(obj),
        "kv" => norm_kv(obj),
        _ => None,
    }
}

/// Validate and clamp a raw JSON object into a clean [`CardSpec`].
pub fn normalize_card(raw: &Value) -> AppResult<CardSpec> {
    let obj = raw
        .as_object()
        .ok_or_else(|| AppError::invalid("Аргументы карточки должны быть объектом"))?;
    let title = clamp_str(obj.get("title"), MAX_TITLE);
    if title.is_empty() {
        return Err(AppError::invalid("У карточки должен быть заголовок (title)"));
    }
    let raw_blocks = obj
        .get("blocks")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::invalid("У карточки должен быть массив blocks"))?;
    let mut blocks = Vec::new();
    for b in raw_blocks {
        if blocks.len() >= MAX_BLOCKS {
            break;
        }
        if let Some(nb) = norm_block(b) {
            blocks.push(nb);
        }
    }
    if blocks.is_empty() {
        return Err(AppError::invalid("В карточке нет ни одного корректного блока"));
    }
    let subtitle = opt_str(obj.get("subtitle"), MAX_SUBTITLE);
    Ok(CardSpec { title, subtitle, blocks })
}

/// Build the overlay draw item conforming to the overlay window contract:
/// `{ "kind": "card", "id": "...", "anchor": {x,y,w,h}?, "spec": CardSpec }`.
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

/// Short Russian summary for tool output and chat rows.
pub fn card_summary(spec: &CardSpec) -> String {
    let n = spec.blocks.len();
    let word = match (n % 10, n % 100) {
        (1, k) if k != 11 => "блок",
        (2..=4, k) if !(12..=14).contains(&k) => "блока",
        _ => "блоков",
    };
    format!("Карточка «{}» ({n} {word})", spec.title)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_title_or_empty_blocks() {
        assert!(normalize_card(&json!({})).is_err());
        assert!(normalize_card(&json!({ "title": "   ", "blocks": [{ "kind": "text", "text": "x" }] })).is_err());
        assert!(normalize_card(&json!({ "title": "Заголовок", "blocks": [] })).is_err());
        assert!(normalize_card(&json!({ "title": "Заголовок", "blocks": [{ "kind": "unknown" }] })).is_err());
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
        match &spec.blocks[6] {
            Block::Flow { direction, nodes, edges } => {
                assert_eq!(direction.as_deref(), Some("lr"));
                assert_eq!(nodes.len(), 2);
                assert_eq!(edges.len(), 1);
            }
            _ => panic!("expected flow"),
        }
        match &spec.blocks[7] {
            Block::Quiz { correct, .. } => assert_eq!(*correct, 2),
            _ => panic!("expected quiz"),
        }
    }

    #[test]
    fn normalizes_compare_math_definition_callout_kv() {
        let raw = json!({
            "title": "Второй набор",
            "blocks": [
                { "kind": "compare", "columns": [{ "title": "A", "pros": ["Быстро"], "cons": ["Дорого"] }, { "title": "B", "items": [{ "text": "Просто", "kind": "pro" }] }] },
                { "kind": "math", "lines": [{ "expr": "2x = 8", "note": "делим на 2" }], "result": "x = 4" },
                { "kind": "definition", "term": "Кэш", "meaning": "Быстрая память рядом", "example": "L1 в процессоре" },
                { "kind": "callout", "tone": "unknown", "text": "Запомни это" },
                { "kind": "kv", "items": [{ "key": "Порт", "value": 443 }] }
            ]
        });
        let spec = normalize_card(&raw).unwrap();
        assert_eq!(spec.blocks.len(), 5);
        assert!(matches!(&spec.blocks[0], Block::Compare { columns } if columns.len() == 2 && columns[0].items.len() == 2));
        assert!(matches!(&spec.blocks[3], Block::Callout { tone, .. } if tone == "idea"));
        assert!(matches!(&spec.blocks[4], Block::Kv { items } if items[0].value == "443"));
    }

    #[test]
    fn clamps_oversized_blocks_rows_and_unicode_strings() {
        let big_title = "Ж".repeat(200);
        let many_blocks: Vec<Value> = (0..15).map(|i| json!({ "kind": "text", "text": format!("Блок {i}") })).collect();
        let spec = normalize_card(&json!({ "title": big_title, "blocks": many_blocks })).unwrap();
        assert_eq!(spec.title.chars().count(), MAX_TITLE);
        assert!(spec.title.ends_with('…'));
        assert_eq!(spec.blocks.len(), MAX_BLOCKS);

        let many_rows: Vec<Value> = (0..25).map(|i| json!([format!("r{i}"), i])).collect();
        let t = normalize_card(&json!({
            "title": "Таблица",
            "blocks": [{ "kind": "table", "headers": ["A", "B"], "rows": many_rows }]
        }))
        .unwrap();
        match &t.blocks[0] {
            Block::Table { rows, .. } => assert_eq!(rows.len(), MAX_TABLE_ROWS),
            _ => panic!("expected table"),
        }
    }

    #[test]
    fn overlay_item_matches_contract() {
        let spec = normalize_card(&json!({
            "title": "DNS",
            "blocks": [{ "kind": "text", "text": "Кратко" }]
        }))
        .unwrap();
        let item = overlay_item("card-1", &spec, Some(AnchorRect { x: 10, y: 20, w: 300, h: 40 }));
        assert_eq!(item["kind"], "card");
        assert_eq!(item["id"], "card-1");
        assert_eq!(item["anchor"], json!({ "x": 10, "y": 20, "w": 300, "h": 40 }));
        assert_eq!(item["spec"]["title"], "DNS");

        let unanchored = overlay_item("card-2", &spec, None);
        assert!(unanchored.get("anchor").is_none());
    }
}
