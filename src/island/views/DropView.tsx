import { Icon } from "../../ui/Icon";
import { useIsland } from "../../store/island";

export function DropView() {
  const toTray = useIsland((s) => s.peekDeck === "tray" && s.hovered);
  return (
    <div className="view view-drop">
      <div className="drop-zone">
        <Icon name={toTray ? "tray" : "drop"} size={20} />
        <div className="drop-zone-copy">
          <strong>{toTray ? "Отпустите, чтобы положить на Полку" : "Отпустите файлы на остров"}</strong>
          <span>Передать Джарвису, обработать или сохранить на Полку</span>
        </div>
      </div>
    </div>
  );
}
