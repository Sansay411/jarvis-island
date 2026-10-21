import { Icon } from "../../ui/Icon";
import { hasActiveMedia, useSystem } from "../../store/system";
import { useIsland } from "../../store/island";
import { usePrefs } from "../../store/prefs";
import { MediaCard } from "./MediaCard";
import { QuickControls } from "./QuickControls";

export function PeekView() {
  const media = useSystem((s) => s.media);
  const set = useIsland((s) => s.set);
  const withMedia = hasActiveMedia(media);
  const shortcut = usePrefs((p) => p.workspaceShortcut);

  return (
    <div className="view view-peek">
      <div className="brow">
        <span className="brow-name">Джарвис</span>
        <div className="brow-actions">
          <button className="icon-btn" aria-label="Открыть чат" onClick={() => set({ workspaceOpen: true })}>
            <Icon name="chat" size={16} />
          </button>
        </div>
      </div>
      {withMedia ? (
        <div className="peek-grid">
          <MediaCard media={media} />
          <QuickControls />
        </div>
      ) : (
        <div className="peek-grid peek-grid-solo">
          <button className="ask-pill" onClick={() => set({ workspaceOpen: true })}>
            Спросить Джарвиса
            {shortcut && <kbd>{shortcut.replaceAll("+", " ")}</kbd>}
          </button>
          <QuickControls />
        </div>
      )}
    </div>
  );
}
