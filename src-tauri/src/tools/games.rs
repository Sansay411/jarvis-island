//! Agent tool `game_play`: play Chess, Russian Checkers or Battleship with the
//! user, inspect the live board and rivalry score, make spoken moves, offer
//! hints or switch JARVIS's table manner (joker / rival / business / auto).

use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};
use tauri::Manager;

use super::{handler, low, object, parse, str_arg, Tool, ToolCtx, ToolOutput};
use crate::error::{AppError, AppResult};
use crate::games::{game_title_ru, mood_title_ru, GamesHub, RemoteGameCommand};

#[derive(Deserialize)]
struct GameArgs {
    action: String,
    game: Option<String>,
    difficulty: Option<String>,
    mood: Option<String>,
    move_str: Option<String>,
}

async fn game_play(ctx: ToolCtx, args: Value) -> AppResult<ToolOutput> {
    let a: GameArgs = parse(args)?;
    let hub = ctx.app.state::<Arc<GamesHub>>().inner().clone();

    match a.action.as_str() {
        "status" => {
            let snap = hub.snapshot();
            let stats = hub.stats();
            let summary = match snap.game.as_deref().filter(|_| snap.open) {
                Some(g) => format!(
                    "Идёт партия «{}» (ход №{}, {}): {}",
                    game_title_ru(g),
                    snap.move_count,
                    snap.score_summary,
                    snap.status_line
                ),
                None => format!(
                    "Счёт партий (игрок:Джарвис) — Шахматы {}:{}, Шашки {}:{}, Морской бой {}:{}",
                    stats.chess.wins, stats.chess.losses,
                    stats.checkers.wins, stats.checkers.losses,
                    stats.battleship.wins, stats.battleship.losses,
                ),
            };
            Ok(ToolOutput::ok(summary, json!({ "snapshot": snap, "stats": stats })))
        }
        "open" | "new_game" => {
            let reply = hub
                .execute_remote(RemoteGameCommand {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    action: a.action.clone(),
                    game: a.game.clone(),
                    difficulty: a.difficulty.clone(),
                    mood: a.mood.clone(),
                    move_str: None,
                })
                .await?;
            Ok(ToolOutput {
                ok: reply.ok,
                summary: reply.summary,
                data: reply.data,
                images: Vec::new(),
            })
        }
        "move" => {
            let mv = a
                .move_str
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| AppError::invalid("Укажите ход в move_str (например «e2e4», «Кf3», «c3-d4» или «Б5»)"))?
                .to_string();
            let reply = hub
                .execute_remote(RemoteGameCommand {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    action: "move".into(),
                    game: a.game,
                    difficulty: None,
                    mood: a.mood,
                    move_str: Some(mv),
                })
                .await?;
            Ok(ToolOutput {
                ok: reply.ok,
                summary: reply.summary,
                data: reply.data,
                images: Vec::new(),
            })
        }
        "hint" => {
            let reply = hub
                .execute_remote(RemoteGameCommand {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    action: "hint".into(),
                    game: a.game,
                    difficulty: None,
                    mood: None,
                    move_str: None,
                })
                .await?;
            Ok(ToolOutput {
                ok: reply.ok,
                summary: reply.summary,
                data: reply.data,
                images: Vec::new(),
            })
        }
        "set_mood" => {
            let mood = a
                .mood
                .as_deref()
                .filter(|m| matches!(*m, "auto" | "joker" | "business" | "rival"))
                .ok_or_else(|| AppError::invalid("Манера должна быть auto, joker, business или rival"))?
                .to_string();
            let reply = hub
                .execute_remote(RemoteGameCommand {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    action: "set_mood".into(),
                    game: None,
                    difficulty: None,
                    mood: Some(mood.clone()),
                    move_str: None,
                })
                .await?;
            let summary = if reply.summary.is_empty() {
                format!("Манера за игровым столом: {}", mood_title_ru(&mood))
            } else {
                reply.summary
            };
            Ok(ToolOutput::ok(summary, reply.data))
        }
        "resign" => {
            let reply = hub
                .execute_remote(RemoteGameCommand {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    action: "resign".into(),
                    game: None,
                    difficulty: None,
                    mood: None,
                    move_str: None,
                })
                .await?;
            Ok(ToolOutput {
                ok: reply.ok,
                summary: reply.summary,
                data: reply.data,
                images: Vec::new(),
            })
        }
        other => Err(AppError::invalid(format!("Неизвестное действие game_play: {other}"))),
    }
}

fn game_title(a: &Value) -> String {
    match str_arg(a, "action") {
        "open" => match str_arg(a, "game") {
            "" => "Открыть игровой клуб".into(),
            g => format!("Играть в «{}»", game_title_ru(g)),
        },
        "new_game" => "Новая партия".into(),
        "move" => format!("Ход в партии: {}", str_arg(a, "move_str")),
        "hint" => "Подсказать ход".into(),
        "set_mood" => format!("Манера игрока: {}", mood_title_ru(str_arg(a, "mood"))),
        "resign" => "Завершить партию".into(),
        _ => "Состояние партии".into(),
    }
}

pub fn tools() -> Vec<Tool> {
    vec![Tool {
        name: "game_play",
        description: "Play board games with the user inside JARVIS's built-in Games Club window: \
            `chess` (Шахматы FIDE), `checkers` (Русские шашки 8×8 with flying kings and backward captures), \
            `battleship` (Морской бой 10×10). Actions: \
            `open` (open the games window; optional `game`: chess|checkers|battleship, `difficulty`: easy|medium|hard, `mood`: auto|joker|business|rival), \
            `new_game` (start a fresh match or rematch), \
            `move` (play the user's spoken/typed move on the board via `move_str`, e.g. 'e2e4', 'конь на f3', 'рокировка', 'c3-d4', 'Б5', and automatically get JARVIS's board reply in the returned data so you can comment on both moves live!), \
            `hint` (highlight and return a strong suggested move for the user), \
            `set_mood` (switch JARVIS's player manner at the table: `joker` = funny/laughing banter, `business` = calm analytical club pro, `rival` = competitive playful trash-talk, `auto` = user's persona settings), \
            `resign` (user resigns the current game), \
            `status` (read current board position and all-time head-to-head scoreboard). \
            Always comment on the game naturally in your own live voice — never sound scripted!",
        schema: object(
            json!({
                "action": { "type": "string", "enum": ["open", "new_game", "move", "hint", "set_mood", "resign", "status"] },
                "game": { "type": "string", "enum": ["chess", "checkers", "battleship"] },
                "difficulty": { "type": "string", "enum": ["easy", "medium", "hard"] },
                "mood": { "type": "string", "enum": ["auto", "joker", "business", "rival"] },
                "move_str": { "type": "string" }
            }),
            &["action"],
        ),
        risk: low,
        title: game_title,
        timeout: Duration::from_secs(15),
        parallel_safe: false,
        handler: handler(game_play),
    }]
}
