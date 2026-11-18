// NotchNook-inspired `Tray` Holding Shelf (`Полка`).
// Keeps dropped files, text snippets, and links pinned inside the notch for
// instant 1-click handoff into Chat, TTS Read-Aloud, Explorer, or Clipboard.

import { useState, type FormEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Icon } from "../../ui/Icon";
import { isTauri } from "../../lib/ipc";
import { useAgent } from "../../store/agent";
import { islandInputHandlers, useIsland } from "../../store/island";
import { useHub } from "../../store/hub";
import { useTts } from "../../store/tts";
import type { ShelfItem } from "../../lib/types";

function fmtBytes(bytes: number | null): string {
  if (!bytes || bytes <= 0) return "";
  if (bytes < 1024) return `${bytes} Б`;
  const units = ["КБ", "МБ", "ГБ"];
  let v = bytes / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(v < 10 ? 1 : 0).replace(".", ",")} ${units[i]}`;
}

function kindBadge(kind: string): string {
  if (kind === "file") return "ФАЙЛ";
  if (kind === "url") return "ССЫЛКА";
  return "ТЕКСТ";
}

export function TrayDeck() {
  const shelf = useHub((s) => s.shelf);
  const shelfAddText = useHub((s) => s.shelfAddText);
  const shelfRemove = useHub((s) => s.shelfRemove);
  const shelfClear = useHub((s) => s.shelfClear);
  const speak = useTts((s) => s.speak);

  const [snippet, setSnippet] = useState("");
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const inputProps = islandInputHandlers();

  const onAddText = async (e: FormEvent) => {
    e.preventDefault();
    if (!snippet.trim()) return;
    await shelfAddText(snippet);
    setSnippet("");
  };

  const copyValue = async (item: ShelfItem) => {
    try {
      await navigator.clipboard.writeText(item.value);
      setCopiedId(item.id);
      window.setTimeout(() => setCopiedId((cur) => (cur === item.id ? null : cur)), 1300);
    } catch {
      /* ignore */
    }
  };

  const sendToChat = (item: ShelfItem) => {
    const island = useIsland.getState();
    if (item.kind === "file") {
      island.addAttachments([{ path: item.value, name: item.label }]);
      island.set({ workspaceOpen: true });
    } else {
      island.set({ workspaceOpen: true });
      if (!useAgent.getState().activeTurn) {
        void useAgent.getState().send(`Разбери этот материал с полки:\n\n${item.value}`, []);
      }
    }
  };

  const revealInFolder = (item: ShelfItem) => {
    if (!isTauri() || item.kind !== "file") return;
    void invoke("files_action", { paths: [item.value], action: "reveal" });
  };

  return (
    <div className="tray-deck">
      <div className="tray-head">
        <div className="tray-title-group">
          <span className="tray-title">Полка</span>
          <span className="tray-count">{shelf.length}</span>
        </div>

        <form className="tray-quick-input" onSubmit={(e) => void onAddText(e)}>
          <Icon name="plus" size={11} />
          <input
            type="text"
            value={snippet}
            onChange={(e) => setSnippet(e.target.value)}
            placeholder="Отложить текст или ссылку…"
            aria-label="Добавить заметку на полку"
            {...inputProps}
          />
        </form>

        {shelf.length > 0 && (
          <button type="button" className="tray-clear-btn" onClick={() => void shelfClear()}>
            Очистить
          </button>
        )}
      </div>

      {shelf.length === 0 ? (
        <div className="tray-empty">
          <Icon name="drop" size={18} />
          <div className="tray-empty-text">
            <strong>Перетащите файлы сюда или вставьте фрагмент выше</strong>
            <span>Материалы остаются под рукой между окнами и в 1 клик передаются Джарвису</span>
          </div>
        </div>
      ) : (
        <div className="tray-list scroll">
          {shelf.map((item) => {
            const sizeStr = fmtBytes(item.sizeBytes);
            const metaParts = [
              kindBadge(item.kind),
              sizeStr,
              !item.exists && item.kind === "file" ? "не найден" : item.subtitle,
            ].filter(Boolean);
            return (
              <div key={item.id} className={`tray-row${!item.exists && item.kind === "file" ? " is-missing" : ""}`}>
                <div className={`tray-glyph is-${item.kind}`}>
                  <Icon name={item.kind === "file" ? "file" : item.kind === "url" ? "globe" : "clipboard"} size={15} />
                </div>

                <div className="tray-item-body">
                  <div className="tray-item-label" title={item.value}>
                    {item.label}
                  </div>
                  <div className="tray-item-meta">{metaParts.join(" · ")}</div>
                </div>

                <div className="tray-actions">
                  <button
                    type="button"
                    className="tray-act-btn"
                    title="Передать в чат Джарвису"
                    onClick={() => sendToChat(item)}
                  >
                    В чат
                  </button>
                  {item.kind !== "file" && (
                    <button
                      type="button"
                      className="tray-act-icon"
                      title="Озвучить вслух"
                      aria-label="Озвучить"
                      onClick={() => void speak(item.value, { interrupt: true })}
                    >
                      <Icon name="speaker" size={13} />
                    </button>
                  )}
                  {item.kind === "file" && (
                    <button
                      type="button"
                      className="tray-act-icon"
                      title="Показать в папке"
                      aria-label="Показать в папке"
                      onClick={() => revealInFolder(item)}
                    >
                      <Icon name="folder" size={13} />
                    </button>
                  )}
                  <button
                    type="button"
                    className="tray-act-icon"
                    title={item.kind === "file" ? "Скопировать путь" : "Скопировать"}
                    aria-label="Скопировать"
                    onClick={() => void copyValue(item)}
                  >
                    <Icon name={copiedId === item.id ? "check" : "copy"} size={13} />
                  </button>
                  <button
                    type="button"
                    className="tray-act-icon is-danger"
                    title="Убрать с полки"
                    aria-label="Убрать"
                    onClick={() => void shelfRemove(item.id)}
                  >
                    <Icon name="close" size={12} />
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
