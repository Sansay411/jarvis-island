use tauri::AppHandle;
pub(super) fn probe_mic_in_child(_input: &str) -> bool {
    false
}
pub(super) fn relaunch(app: &AppHandle) {
    app.restart();
}
