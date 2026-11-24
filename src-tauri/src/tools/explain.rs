//! Agent tools for showing and clearing visual explanation cards in the
//! desktop overlay (and inline in the workspace chat transcript).

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::{json, Value};

use super::{handler, low, object, str_arg, Tool, ToolCtx, ToolOutput};
use crate::cards::{self, AnchorRect};
use crate::error::AppResult;
use crate::platform::uia;

static NEXT_CARD: AtomicU64 = AtomicU64::new(1);

/// Resolve an optional `anchor` argument into physical screen pixels.
/// Supports:
/// - `{ snapshot_id, element_id }` (UI Automation element from `ui_inspect`)
/// - `{ x, y, w?, h?, space: "image", capture_id? }` (screenshot pixels from `screen_capture`)
/// - `{ x, y, w?, h?, space?: "screen" }` (physical screen pixels)
/// Returns `(Option<AnchorRect>, Option<String>)` where the second value is a
/// non-fatal warning when the anchor reference was stale or incomplete.
fn resolve_anchor(raw: Option<&Value>) -> (Option<AnchorRect>, Option<String>) {
    let Some(val) = raw else {
        return (None, None);
    };
    let Some(obj) = val.as_object() else {
        return (None, Some("anchor должен быть объектом — карточка показана без привязки".into()));
    };

    if let (Some(snap), Some(el)) = (
        obj.get("snapshot_id").or_else(|| obj.get("snapshotId")).and_then(Value::as_u64),
        obj.get("element_id").or_else(|| obj.get("elementId")).and_then(Value::as_u64),
    ) {
        return match uia::element_rect(snap, el as usize) {
            Some(r) => (Some(AnchorRect { x: r.x, y: r.y, w: r.width.max(0), h: r.height.max(0) }), None),
            None => (None, Some("Элемент из ui_inspect устарел — карточка показана без привязки".into())),
        };
    }

    let x = obj.get("x").and_then(Value::as_f64);
    let y = obj.get("y").and_then(Value::as_f64);
    let (Some(x), Some(y)) = (x, y) else {
        return (None, Some("В anchor не указаны координаты x/y — карточка показана без привязки".into()));
    };
    if !x.is_finite() || !y.is_finite() {
        return (None, Some("Некорректные координаты anchor — карточка показана без привязки".into()));
    }

    let w = obj
        .get("w")
        .or_else(|| obj.get("width"))
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite() && *v >= 0.0)
        .unwrap_or(0.0);
    let h = obj
        .get("h")
        .or_else(|| obj.get("height"))
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite() && *v >= 0.0)
        .unwrap_or(0.0);

    let capture_id = obj.get("capture_id").or_else(|| obj.get("captureId")).and_then(Value::as_u64);
    let space = obj
        .get("space")
        .and_then(Value::as_str)
        .unwrap_or(if capture_id.is_some() { "image" } else { "screen" });

    match super::screen::rect_to_screen(x, y, w, h, space, capture_id) {
        Ok(r) => (Some(AnchorRect { x: r.x, y: r.y, w: r.width.max(0), h: r.height.max(0) }), None),
        Err(e) => (None, Some(format!("{e} — карточка показана без привязки"))),
    }
}

async fn explain_card(ctx: ToolCtx, args: Value) -> AppResult<ToolOutput> {
    let spec = cards::normalize_card(&args)?;
    let keep = args.get("keep").and_then(Value::as_bool).unwrap_or(false);
    let (anchor, anchor_warning) = resolve_anchor(args.get("anchor"));
    let id = format!("card-{}", NEXT_CARD.fetch_add(1, Ordering::Relaxed));
    let item = cards::overlay_item(&id, &spec, anchor);

    crate::overlay::draw(&ctx.app, vec![item], !keep)?;

    // The model already has the card in its own call: echo only what it
    // cannot know (what survived validation, where it went).
    let kinds: Vec<&str> = spec.blocks.iter().map(cards::Block::kind).collect();
    let mut data = json!({
        "id": id,
        "title": spec.title,
        "blocks": kinds,
        "replaced": !keep,
        "anchored": anchor.is_some(),
    });
    if let Some(w) = anchor_warning {
        data["anchorWarning"] = json!(w);
    }
    Ok(ToolOutput::ok(cards::card_summary(&spec), data))
}

async fn explain_clear(ctx: ToolCtx, _args: Value) -> AppResult<ToolOutput> {
    crate::overlay::clear(&ctx.app)?;
    Ok(ToolOutput::ok("Карточки убраны с экрана", json!({ "cleared": true })))
}

fn card_title(args: &Value) -> String {
    let t = str_arg(args, "title").trim();
    if t.is_empty() {
        return "Пояснительная карточка".into();
    }
    let short: String = t.chars().take(44).collect();
    if t.chars().count() > 44 {
        format!("Карточка «{short}…»")
    } else {
        format!("Карточка «{short}»")
    }
}

fn card_schema() -> Value {
    object(
        json!({
            "title": { "type": "string", "description": "Short clear card heading (max 120 chars)" },
            "subtitle": { "type": "string", "description": "Optional one-line context below the title" },
            "keep": { "type": "boolean", "description": "Set true to keep existing cards on screen; default false replaces them" },
            "anchor": {
                "type": "object",
                "description": "Optional region the card is placed beside: {snapshot_id, element_id} of a ui_inspect element, {x, y, w?, h?, space: 'image', capture_id?} in screenshot pixels from screen_capture, or physical screen pixels {x, y, w?, h?, space: 'screen'}. Without it the card goes to the default corner.",
                "properties": {
                    "x": { "type": "number" },
                    "y": { "type": "number" },
                    "w": { "type": "number" },
                    "h": { "type": "number" },
                    "space": { "type": "string", "enum": ["screen", "image"] },
                    "capture_id": { "type": "integer" },
                    "snapshot_id": { "type": "integer" },
                    "element_id": { "type": "integer" }
                }
            },
            "blocks": {
                "type": "array",
                "minItems": 1,
                "maxItems": 8,
                "description": "1 to 8 visual blocks, best 1 to 4. Write all texts in the user's language, short. Each object has `kind`:\n\
                    - text: {kind:'text', text} (short markdown: **bold**, *italic*, `code`, lists)\n\
                    - steps: {kind:'steps', items:[{title, note?}]}\n\
                    - table: {kind:'table', headers:[...], rows:[[...]] (<=12 rows, <=6 columns), highlightCol?: 0-based column to stress}\n\
                    - bars: {kind:'bars', title?, unit?, items:[{label, value: number, highlight?}]} (<=10 bars)\n\
                    - line: {kind:'line', title?, unit?, xLabels:[...], series:[{name?, values:[...]}]}\n\
                    - timeline: {kind:'timeline', items:[{date, label, note?}]}\n\
                    - flow: {kind:'flow', direction?:'lr'|'td', nodes:[{id, label, note?, tone?:'default'|'accent'|'muted'}], edges:[{from: node id, to: node id, label?}]} (<=12 nodes, laid out automatically)\n\
                    - compare: {kind:'compare', columns:[{title, subtitle?, accent?, items:[{text, kind?:'pro'|'con'|'neutral'}]}]}\n\
                    - math: {kind:'math', lines:[{expr, note?}], result?} (plain text maths: x^2, sqrt(x), <=, *; one transformation per line)\n\
                    - definition: {kind:'definition', term, meaning, example?}\n\
                    - quiz: {kind:'quiz', question, options:[2-6 strings], correct: 0-based index of the right option, explanation?} (the user clicks an option and sees right/wrong locally)\n\
                    - callout: {kind:'callout', tone?:'idea'|'tip'|'warning', title?, text}\n\
                    - kv: {kind:'kv', items:[{key, value}]}",
                "items": {
                    "type": "object",
                    "properties": {
                        "kind": {
                            "type": "string",
                            "enum": ["text", "steps", "table", "bars", "line", "timeline", "flow", "compare", "math", "definition", "quiz", "callout", "kv"]
                        }
                    },
                    "required": ["kind"]
                }
            }
        }),
        &["title", "blocks"],
    )
}

pub fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "explain_card",
            description: "Show a compact visual explanation card beside what the user is reading (also shown in the chat). \
                Use when a picture explains faster than prose: a diagram of a process, a table, a chart, a timeline, a comparison, \
                numbered steps, a worked formula, a definition or a one-question quiz. Pass `anchor` to place it next to the thing \
                being explained. Keep the spoken/text reply to one or two sentences when a card is shown.",
            schema: card_schema(),
            risk: low,
            title: card_title,
            timeout: Duration::from_secs(5),
            parallel_safe: false,
            handler: handler(explain_card),
        },
        Tool {
            name: "explain_clear",
            description: "Clear explanation cards currently shown on the screen overlay.",
            schema: object(json!({}), &[]),
            risk: low,
            title: |_| "Убрать карточки с экрана".into(),
            timeout: Duration::from_secs(5),
            parallel_safe: false,
            handler: handler(explain_clear),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Risk;

    #[test]
    fn resolves_screen_anchor_and_handles_invalid_gracefully() {
        let (rect, warn) = resolve_anchor(Some(&json!({ "x": 120, "y": 240, "w": 400, "h": 80 })));
        assert_eq!(rect, Some(AnchorRect { x: 120, y: 240, w: 400, h: 80 }));
        assert!(warn.is_none());

        let (rect2, warn2) = resolve_anchor(Some(&json!({ "x": 10.4, "y": 20.6, "width": 50, "height": 30, "space": "screen" })));
        assert_eq!(rect2, Some(AnchorRect { x: 10, y: 21, w: 50, h: 30 }));
        assert!(warn2.is_none());

        let (rect3, warn3) = resolve_anchor(Some(&json!({ "snapshot_id": 999999, "element_id": 0 })));
        assert!(rect3.is_none());
        assert!(warn3.is_some());

        let (rect4, warn4) = resolve_anchor(None);
        assert!(rect4.is_none());
        assert!(warn4.is_none());
    }

    #[test]
    fn tool_specs_have_low_risk_and_russian_titles() {
        let list = tools();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "explain_card");
        assert_eq!((list[0].risk)(&json!({})), Risk::Low);
        assert_eq!((list[0].title)(&json!({ "title": "Как работает DNS" })), "Карточка «Как работает DNS»");
        assert_eq!(list[1].name, "explain_clear");
        assert_eq!((list[1].risk)(&json!({})), Risk::Low);
    }
}
