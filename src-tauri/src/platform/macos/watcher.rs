use crate::platform::island::Island;
use crate::platform::winmgr::ForegroundHistory;
use std::sync::Arc;
pub fn spawn(island: Arc<Island>, history: Arc<ForegroundHistory>) {
    let weak = Arc::downgrade(&island);
    if let Err(e) = std::thread::Builder::new()
        .name("macos-foreground".into())
        .spawn(move || {
            let mut last = None;
            while weak.upgrade().is_some() {
                if let Some(info) = super::winmgr::foreground_window() {
                    if last != Some(info.hwnd) {
                        last = Some(info.hwnd);
                        history.record_info(info);
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        })
    {
        tracing::warn!("Не удалось запустить наблюдение за окнами: {e}");
    }
}
