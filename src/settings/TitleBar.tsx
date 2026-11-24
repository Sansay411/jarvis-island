// The settings window has no Windows title bar (its grey strip clashed with
// the obsidian page): this transparent band drags the window, a double
// click maximises it, and three quiet buttons sit on the right.

import { getCurrentWindow } from "@tauri-apps/api/window";
import { hasTauri } from "../lib/workspace-ipc";

export function TitleBar({
  onClose,
  allowMaximize = true,
}: {
  onClose?: () => void;
  allowMaximize?: boolean;
} = {}) {
  const withWin = (fn: (w: ReturnType<typeof getCurrentWindow>) => Promise<void>) => {
    if (!hasTauri()) return;
    try {
      void fn(getCurrentWindow());
    } catch {
      /* ignore in browser preview */
    }
  };

  const handleClose = () => {
    if (onClose) {
      onClose();
      return;
    }
    withWin((w) => w.close());
  };

  return (
    <div className="titlebar" data-tauri-drag-region>
      <div className="titlebar-buttons">
        <button type="button" className="titlebar-btn" aria-label="Свернуть" onClick={() => withWin((w) => w.minimize())}>
          <svg viewBox="0 0 10 10" aria-hidden="true">
            <path d="M1.5 5h7" />
          </svg>
        </button>
        {allowMaximize && (
          <button type="button" className="titlebar-btn" aria-label="Развернуть" onClick={() => withWin((w) => w.toggleMaximize())}>
            <svg viewBox="0 0 10 10" aria-hidden="true">
              <rect x="1.5" y="1.5" width="7" height="7" rx="1.5" />
            </svg>
          </button>
        )}
        <button type="button" className="titlebar-btn is-close" aria-label="Закрыть" onClick={handleClose}>
          <svg viewBox="0 0 10 10" aria-hidden="true">
            <path d="m2 2 6 6M8 2 2 8" />
          </svg>
        </button>
      </div>
    </div>
  );
}
