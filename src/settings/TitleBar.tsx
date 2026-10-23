// The settings window has no Windows title bar (its grey strip clashed with
// the obsidian page): this transparent band drags the window, a double
// click maximises it, and three quiet buttons sit on the right.

import { getCurrentWindow } from "@tauri-apps/api/window";

export function TitleBar() {
  const win = getCurrentWindow();
  return (
    <div className="titlebar" data-tauri-drag-region>
      <div className="titlebar-buttons">
        <button className="titlebar-btn" aria-label="Свернуть" onClick={() => void win.minimize()}>
          <svg viewBox="0 0 10 10" aria-hidden="true">
            <path d="M1.5 5h7" />
          </svg>
        </button>
        <button className="titlebar-btn" aria-label="Развернуть" onClick={() => void win.toggleMaximize()}>
          <svg viewBox="0 0 10 10" aria-hidden="true">
            <rect x="1.5" y="1.5" width="7" height="7" rx="1.5" />
          </svg>
        </button>
        <button className="titlebar-btn is-close" aria-label="Закрыть" onClick={() => void win.close()}>
          <svg viewBox="0 0 10 10" aria-hidden="true">
            <path d="m2 2 6 6M8 2 2 8" />
          </svg>
        </button>
      </div>
    </div>
  );
}
