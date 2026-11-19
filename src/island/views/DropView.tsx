import { motion, useReducedMotion } from "motion/react";
import { Icon } from "../../ui/Icon";
import { useIsland } from "../../store/island";

/** A file is dragged over the island: drop on the left to work with it
 *  (or onto the shelf), on the right to send it to Telegram «Избранное». */
export function DropView() {
  const toTray = useIsland((s) => s.peekDeck === "tray" && s.hovered);
  const onSend = useIsland((s) => s.dropOnSend);
  const reduced = useReducedMotion();
  return (
    <div className="view view-drop">
      <div className={`drop-zone${onSend ? " is-dim" : ""}`}>
        <Icon name={toTray ? "tray" : "drop"} size={20} />
        <div className="drop-zone-copy">
          <strong>{toTray ? "Отпустите, чтобы положить на Полку" : "Отпустите файлы на остров"}</strong>
          <span>Передать Джарвису, обработать или сохранить на Полку</span>
        </div>
      </div>
      <div data-drop-target="telegram" className={`drop-send${onSend ? " is-armed" : ""}`}>
        <motion.span
          className="drop-send-icon"
          key={onSend ? "armed" : "idle"}
          initial={reduced || !onSend ? false : { scale: 1 }}
          animate={reduced || !onSend ? undefined : { scale: [1, 1.14, 1] }}
          transition={{ duration: 0.45, ease: [0.22, 1, 0.36, 1] }}
        >
          <Icon name="send" size={18} />
        </motion.span>
        <strong>В Telegram</strong>
        <span>Избранное</span>
      </div>
    </div>
  );
}
