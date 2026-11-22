pub fn spawn(on_change: impl Fn() + Send + Sync + 'static) {
    if let Err(e) = std::thread::Builder::new()
        .name("macos-clipboard".into())
        .spawn(move || {
            let mut last = super::clipboard::change_count().ok();
            loop {
                std::thread::sleep(std::time::Duration::from_millis(500));
                if let Ok(sequence) = super::clipboard::change_count() {
                    if last != Some(sequence) {
                        last = Some(sequence);
                        on_change();
                    }
                }
            }
        })
    {
        tracing::warn!("Не удалось запустить наблюдение за буфером обмена: {e}");
    }
}
