// The clipboard history: what was copied recently, searchable. A click (or
// Enter) puts the entry on the clipboard and pastes it into the window the
// user was working in — the island never takes that window's focus unless the
// search box needs the keyboard.

import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { Icon } from "../../ui/Icon";
import { useIsland } from "../../store/island";
import { ago, clearClips, closeClips, moveSelection, pasteClip, pinClip, removeClip, selectRow, setQuery, setRecording, useClips, type Clip } from "../../store/clips";

function Row({ clip, index }: { clip: Clip; index: number }) {
  const selected = useClips((s) => s.selected === index);
  const image = clip.kind === "image";
  const meta = [clip.source, ago(clip.usedAt)].filter(Boolean).join(" · ");
  return (
    <div
      role="option"
      aria-selected={selected}
      data-index={index}
      className={`clip-row${selected ? " is-selected" : ""}${image ? " is-image" : ""}`}
      onMouseMove={() => selectRow(index)}
      onClick={() => void pasteClip(clip.id)}
    >
      <div className="clip-glyph">{image && clip.thumb ? <img src={clip.thumb} alt="" /> : <Icon name={clip.kind === "files" ? "file" : "clipboard"} size={15} />}</div>
      <div className="clip-text">
        <span className="clip-preview">{clip.preview}</span>
        <span className="clip-meta">
          {clip.pinned && <Icon name="pin" size={10} className="clip-pin-mark" />}
          {meta}
        </span>
      </div>
      <div className="clip-actions">
        <button
          className="icon-btn"
          aria-label={clip.pinned ? "Открепить" : "Закрепить"}
          title={clip.pinned ? "Открепить" : "Закрепить: не удалится сам"}
          onClick={(e) => {
            e.stopPropagation();
            void pinClip(clip.id, !clip.pinned);
          }}
        >
          <Icon name="pin" size={14} />
        </button>
        <button
          className="icon-btn"
          aria-label="Удалить"
          title="Удалить из истории"
          onClick={(e) => {
            e.stopPropagation();
            void removeClip(clip.id);
          }}
        >
          <Icon name="trash" size={14} />
        </button>
      </div>
    </div>
  );
}

export function ClipsView() {
  const items = useClips((s) => s.items);
  const query = useClips((s) => s.query);
  const selected = useClips((s) => s.selected);
  const loaded = useClips((s) => s.loaded);
  const enabled = useClips((s) => s.enabled);
  const error = useClips((s) => s.error);
  const shortcut = useClips((s) => s.shortcut);
  const keyboard = useIsland((s) => s.clipsKeyboard);
  const cardRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const sureTimer = useRef<number | undefined>(undefined);
  const [sure, setSure] = useState(false);

  // Report the natural height so the island shape fits the panel.
  useLayoutEffect(() => {
    const el = cardRef.current;
    if (!el) return;
    const report = () => useClips.setState({ height: Math.ceil(el.scrollHeight) });
    report();
    const ro = new ResizeObserver(report);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  // The search box takes the keyboard once the native window can give it.
  // No scrolling: the island is still growing, and revealing the box would
  // scroll its clipped frame and leave the header out of sight for good.
  useEffect(() => {
    const focus = () => inputRef.current?.focus({ preventScroll: true });
    window.addEventListener("jarvis:input-ready", focus);
    return () => window.removeEventListener("jarvis:input-ready", focus);
  }, []);

  useEffect(() => () => window.clearTimeout(sureTimer.current), []);

  // The row Enter would paste stays in view. Only the list scrolls:
  // scrollIntoView would also scroll the island's clipped frame while it is
  // still growing and push the header out of sight.
  useEffect(() => {
    const list = listRef.current;
    const row = list?.querySelector<HTMLElement>(`[data-index="${selected}"]`);
    if (!list || !row) return;
    const top = row.offsetTop;
    const bottom = top + row.offsetHeight;
    if (top < list.scrollTop) list.scrollTop = top;
    else if (bottom > list.scrollTop + list.clientHeight) list.scrollTop = bottom - list.clientHeight;
  }, [selected, items]);

  const onKeyDown = (e: KeyboardEvent) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      moveSelection(1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      moveSelection(-1);
    } else if (e.key === "Enter") {
      const { items: list, selected: at } = useClips.getState();
      if (list[at]) {
        e.preventDefault();
        void pasteClip(list[at].id);
      }
    }
  };

  // A click into the search asks for the keyboard; the controller makes the
  // window focusable and the box is focused when it is ready.
  const wake = () => {
    if (!useIsland.getState().clipsKeyboard) useIsland.getState().set({ clipsKeyboard: true });
  };

  const clear = () => {
    window.clearTimeout(sureTimer.current);
    if (!sure) {
      setSure(true);
      sureTimer.current = window.setTimeout(() => setSure(false), 3000);
      return;
    }
    setSure(false);
    void clearClips();
  };

  return (
    <div className="view view-clips" onKeyDown={onKeyDown}>
      <div ref={cardRef} className="clips-card">
        <div className="brow">
          <span className="brow-name">Буфер обмена</span>
          <div className="brow-actions">
            <button
              className="icon-btn"
              aria-label={enabled ? "Поставить запись на паузу" : "Продолжить запись"}
              title={enabled ? "Пауза: новое копирование не сохраняется" : "Продолжить запись"}
              onClick={() => void setRecording(!enabled)}
            >
              <Icon name={enabled ? "pause" : "play"} size={14} />
            </button>
            <button className="icon-btn" aria-label="Закрыть" onClick={closeClips}>
              <Icon name="close" size={14} />
            </button>
          </div>
        </div>
        <label className="clips-search" onMouseDown={wake}>
          <Icon name="search" size={15} />
          <input ref={inputRef} value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Поиск по буферу" spellCheck={false} autoComplete="off" aria-label="Поиск по буферу" />
        </label>
        {!enabled && <div className="clips-note">Запись на паузе: новое не сохраняется. Старое можно вставлять.</div>}
        <div ref={listRef} className="clips-list" role="listbox" aria-label="История буфера обмена">
          {items.map((c, i) => (
            <Row key={c.id} clip={c} index={i} />
          ))}
          {loaded && items.length === 0 && <p className="clips-empty">{query ? "Ничего не нашлось." : enabled ? "Пока пусто. Скопируйте что-нибудь — оно появится здесь." : "Пока пусто."}</p>}
        </div>
        {error && <div className="clips-note is-error">{error}</div>}
        <div className="clips-foot">
          <span className="clips-hint">
            {keyboard ? (
              <>
                <kbd>↑</kbd> <kbd>↓</kbd> выбрать · <kbd>Enter</kbd> вставить · <kbd>Esc</kbd> закрыть
              </>
            ) : (
              shortcut && (
                <>
                  Позвать с клавиатуры: <kbd>{shortcut.replaceAll("+", " ")}</kbd>
                </>
              )
            )}
          </span>
          <button className="file-chip" onClick={clear} title="Закреплённые останутся">
            {sure ? "Точно очистить?" : "Очистить"}
          </button>
        </div>
      </div>
    </div>
  );
}
