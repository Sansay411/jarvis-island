//! Tauri IPC adapters for the `games` window (Chess, Checkers, Battleship).

use std::sync::Arc;

use tauri::State;

use crate::error::AppResult;
use crate::games::{
    ActiveGameSnapshot, AllGameStats, CommentRequest, GamesHub, RemoteGameCommand, RemoteGameReply,
};

#[tauri::command]
pub async fn games_open(
    games: State<'_, Arc<GamesHub>>,
    game: Option<String>,
    difficulty: Option<String>,
    mood: Option<String>,
) -> AppResult<()> {
    let boot = game.map(|g| RemoteGameCommand {
        request_id: uuid::Uuid::new_v4().to_string(),
        action: "open".into(),
        game: Some(g),
        difficulty,
        mood,
        move_str: None,
    });
    games.open_window(boot)?;
    Ok(())
}

#[tauri::command]
pub fn games_ready(games: State<'_, Arc<GamesHub>>) -> Option<RemoteGameCommand> {
    games.mark_ready()
}

#[tauri::command]
pub fn games_stats(games: State<'_, Arc<GamesHub>>) -> AllGameStats {
    games.stats()
}

#[tauri::command]
pub fn games_record_finish(
    games: State<'_, Arc<GamesHub>>,
    game: String,
    winner: String,
) -> AppResult<AllGameStats> {
    games.record_finish(&game, &winner)
}

#[tauri::command]
pub fn games_sync_snapshot(games: State<'_, Arc<GamesHub>>, snapshot: ActiveGameSnapshot) {
    games.sync_snapshot(snapshot);
}

#[tauri::command]
pub fn games_reply_cmd(games: State<'_, Arc<GamesHub>>, reply: RemoteGameReply) {
    games.resolve_remote(reply);
}

#[tauri::command]
pub fn games_comment(games: State<'_, Arc<GamesHub>>, req: CommentRequest) -> u64 {
    games.inner().spawn_comment(req)
}
