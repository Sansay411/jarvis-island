// `Полка`: what the user put aside — files, text snippets, links — as a row of
// tiles with real previews. A file tile can be dragged out of the island into
// Explorer or any window; the big tile on the right sends the chosen file to
// Telegram «Избранное». Everything animates once per event, never in idle.

import { useRef, useState, type FormEvent, type PointerEvent as RPointerEvent, type WheelEvent } from "react";
import { AnimatePresence, motion, useAnimate, useMotionValue, useReducedMotion, useSpring } from "motion/react";
import { invoke } from "@tauri-apps/api/core";
import { Icon } from "../../ui/Icon";
import { ipc, isTauri } from "../../lib/ipc";
import { useAgent } from "../../store/agent";
import { islandInputHandlers, useIsland } from "../../store/island";
import { useHub } from "../../store/hub";
import type { ShelfItem } from "../../lib/types";
import { middleTruncate } from "../motion";
import { useShelfThumb } from "../shelfThumbs";
import { beginDragOut, endDragOut, rowCapturesWheel } from "../dragGuard";
import { runFileAction, showFiles } from "../../store/files";

const TILE_SPRING = { type: "spring", stiffness: 520, damping: 34 } as const;

const GHOSTS: Array<{ icon: "file" | "globe" | "clipboard"; label: string }> = [
  { icon: "file", label: "Файл" },
  { icon: "globe", label: "Ссылка" },
  { icon: "clipboard", label: "Текст" },
];

function sendToChat(item: ShelfItem) {
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
}

function revealInFolder(item: ShelfItem) {
  if (!isTauri() || item.kind !== "file") return;
  void invoke("files_action", { paths: [item.value], action: "reveal" });
}

function extOf(name: string): string {
  const m = /.([a-z0-9]{1,5})$/i.exec(name);
  return m ? m[1]!.toUpperCase() : "";
}

function hostOf(url: string): string {
  try {
    return new URL(url).host.replace(/^www\./, "");
  } catch {
    return url;
  }
}

/** Starts the native drag once the pointer moved 4 px with the button down. */
function useDragOut(item: ShelfItem) {
  const start = useRef<{ x: number; y: number } | null>(null);
  const canDrag = item.kind === "file" && item.exists;
  return {
    onPointerDown: (e: RPointerEvent) => {
      if (!canDrag || e.button !== 0) return;
      start.current = { x: e.clientX, y: e.clientY };
    },
    onPointerMove: (e: RPointerEvent) => {
      const s = start.current;
      if (!s || (e.buttons & 1) === 0) {
        start.current = null;
        return;
      }
      if (Math.hypot(e.clientX - s.x, e.clientY - s.y) < 4) return;
      start.current = null;
      if (!isTauri()) return;
      beginDragOut([item.value]);
      void ipc.hub
        .shelfDragOut([item.value])
        .catch(() => undefined)
        // Events queued during the modal drag loop may still arrive just after it.
        .finally(() => window.setTimeout(endDragOut, 400));
    },
    onPointerUp: () => {
      start.current = null;
    },
  };
}

function ShelfTile({
  item,
  index,
  fresh,
  selected,
  onSelect,
  onRemove,
}: {
  item: ShelfItem;
  index: number;
  /** Put on the shelf while it was open: falls in from above. */
  fresh: boolean;
  selected: boolean;
  onSelect: () => void;
  onRemove: () => void;
}) {
  const missing = item.kind === "file" && !item.exists;
  const thumb = useShelfThumb(item.kind === "file" && item.exists ? item.value : null);
  const drag = useDragOut(item);
  const reduced = useReducedMotion();
  // 3D tilt toward the cursor with a moving glare; springs settle on leave.
  const rx = useSpring(useMotionValue(0), { stiffness: 170, damping: 18, mass: 0.6 });
  const ry = useSpring(useMotionValue(0), { stiffness: 170, damping: 18, mass: 0.6 });
  const onTilt = (e: RPointerEvent<HTMLDivElement>) => {
    drag.onPointerMove(e);
    if (reduced || missing) return;
    const el = e.currentTarget;
    const r = el.getBoundingClientRect();
    const px = (e.clientX - r.left) / r.width;
    const py = (e.clientY - r.top) / r.height;
    ry.set((px - 0.5) * 11);
    rx.set((0.5 - py) * 11);
    el.style.setProperty("--gx", `${(px * 100).toFixed(1)}%`);
    el.style.setProperty("--gy", `${(py * 100).toFixed(1)}%`);
  };
  const onLeave = () => {
    rx.set(0);
    ry.set(0);
  };
  const isFolder = item.kind === "file" && item.sizeBytes == null && !/\.[a-z0-9]{1,5}$/i.test(item.label);

  return (
    <motion.div
      layout
      data-shelf-tile={item.id}
      data-exists={String(!missing)}
      className={`shelf-tile is-${item.kind}${thumb ? " has-thumb" : ""}${missing ? " is-missing" : ""}${selected ? " is-selected" : ""}`}
      style={{ rotateX: rx, rotateY: ry }}
      initial={
        reduced
          ? { opacity: 0 }
          : fresh
            ? { opacity: 0, y: -60, scale: 0.9, rotate: 5 }
            : { opacity: 0, y: 18, scale: 0.78, rotate: -7 }
      }
      animate={{
        opacity: missing ? 0.45 : 1,
        y: 0,
        scale: 1,
        rotate: 0,
        transition: fresh
          ? { type: "spring", stiffness: 380, damping: 15 }
          : { ...TILE_SPRING, delay: reduced ? 0 : Math.min(index, 8) * 0.055 },
      }}
      exit={reduced ? { opacity: 0 } : { opacity: 0, y: 26, scale: 0.5, rotate: 9, transition: { duration: 0.22 } }}
      transition={TILE_SPRING}
      whileHover={reduced || missing ? undefined : { scale: 1.05, y: -3, zIndex: 3 }}
      whileTap={reduced || missing ? undefined : { scale: 0.96 }}
      title={item.value}
      onClick={onSelect}
      onDoubleClick={() => {
        if (item.kind === "file" && !missing && isTauri()) void invoke("files_action", { paths: [item.value], action: "open" });
      }}
      onPointerDown={drag.onPointerDown}
      onPointerUp={drag.onPointerUp}
      onPointerMove={onTilt}
      onPointerLeave={onLeave}
    >
      {selected && (
        <motion.span layoutId="shelf-sel" className="shelf-tile-sel" transition={{ type: "spring", stiffness: 500, damping: 36 }} />
      )}
      <span className="shelf-tile-glare" aria-hidden="true" />
      <div className="shelf-tile-face">
        {thumb ? (
          <motion.img
            src={thumb}
            alt=""
            draggable={false}
            initial={reduced ? false : { opacity: 0, filter: "blur(10px)", scale: 1.12 }}
            animate={{ opacity: 1, filter: "blur(0px)", scale: 1 }}
            transition={{ duration: 0.5, ease: [0.22, 1, 0.36, 1] }}
          />
        ) : item.kind === "text" ? (
          <p className="shelf-tile-text">{item.value}</p>
        ) : item.kind === "url" ? (
          <div className="shelf-tile-glyph">
            <Icon name="globe" size={22} />
            <span>{hostOf(item.value)}</span>
          </div>
        ) : (
          <div className="shelf-tile-glyph">
            <Icon name={isFolder ? "folder" : "file"} size={26} />
          </div>
        )}
      </div>

      {item.kind === "file" && !thumb && !isFolder && extOf(item.label) && (
        <span className="shelf-tile-ext">{extOf(item.label)}</span>
      )}
      <span className="shelf-tile-label">{middleTruncate(item.label, 13)}</span>

      <div className="shelf-tile-actions" onPointerDown={(e) => e.stopPropagation()}>
        <button
          type="button"
          title="Передать в чат Джарвису"
          aria-label="В чат"
          disabled={missing}
          onClick={(e) => {
            e.stopPropagation();
            sendToChat(item);
          }}
        >
          <Icon name="chat" size={11} />
        </button>
        {item.kind === "file" && (
          <button
            type="button"
            title="Показать в папке"
            aria-label="Показать в папке"
            disabled={missing}
            onClick={(e) => {
              e.stopPropagation();
              revealInFolder(item);
            }}
          >
            <Icon name="folder" size={11} />
          </button>
        )}
        <button
          type="button"
          title="Убрать с полки"
          aria-label="Убрать"
          onClick={(e) => {
            e.stopPropagation();
            onRemove();
          }}
        >
          <Icon name="close" size={10} />
        </button>
      </div>
    </motion.div>
  );
}

/** Sends through the file card, like a drop on «В Telegram»: the card keeps
 *  the job and its verified result even if the pointer leaves the island. */
function TelegramTile({ target }: { target: ShelfItem | null }) {
  const enabled = Boolean(target && target.kind === "file" && target.exists);
  const reduced = useReducedMotion();
  const [scope, animate] = useAnimate();
  const [ripple, setRipple] = useState(0);
  const send = async () => {
    if (!target || !enabled || !isTauri()) return;
    const paths = [target.value];
    setRipple((n) => n + 1);
    if (!reduced && scope.current) {
      // The plane takes off up-right, then the file card takes over.
      await animate(scope.current, { x: 46, y: -40, rotate: -18, opacity: 0, scale: 0.7 }, { duration: 0.34, ease: [0.5, 0, 0.75, 0] });
      void animate(scope.current, { x: 0, y: 0, rotate: 0, opacity: 1, scale: 1 }, { duration: 0 });
    }
    void showFiles(paths).then(() => runFileAction("telegram", paths));
  };
  return (
    <motion.button
      type="button"
      className={`shelf-send${enabled ? " is-ready" : ""}`}
      disabled={!enabled}
      title="Отправить выбранный файл в Telegram, в «Избранное»"
      onClick={() => void send()}
      whileHover={enabled && !reduced ? "hover" : undefined}
      whileTap={enabled && !reduced ? { scale: 0.96 } : undefined}
    >
      <AnimatePresence>
        {ripple > 0 && (
          <motion.span
            key={ripple}
            className="shelf-send-ripple"
            initial={{ scale: 0.3, opacity: 0.55 }}
            animate={{ scale: 2.4, opacity: 0 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.7, ease: [0.22, 1, 0.36, 1] }}
          />
        )}
      </AnimatePresence>
      <span className="shelf-send-icon">
        <motion.span
          ref={scope}
          style={{ display: "grid" }}
          variants={{ hover: { x: 2, y: -2, rotate: -10 } }}
          transition={{ type: "spring", stiffness: 420, damping: 18 }}
        >
          <Icon name="plane" size={20} />
        </motion.span>
      </span>
      <strong>В Telegram</strong>
      <span className="shelf-send-caption">{target && target.kind === "file" ? middleTruncate(target.label, 18) : "Нет файла"}</span>
    </motion.button>
  );
}

export function TrayDeck() {
  const shelf = useHub((s) => s.shelf);
  const shelfAddText = useHub((s) => s.shelfAddText);
  const shelfRemove = useHub((s) => s.shelfRemove);
  const shelfClear = useHub((s) => s.shelfClear);

  const [snippet, setSnippet] = useState("");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  // Items present when the deck opened cascade in; later ones fall from above.
  const seenAtOpen = useRef<Set<string>>(new Set(shelf.map((i) => i.id)));
  const inputProps = islandInputHandlers();

  // The Telegram tile sends the chosen tile, or the newest existing file.
  const selected =
    shelf.find((i) => i.id === selectedId) ?? shelf.find((i) => i.kind === "file" && i.exists) ?? null;

  const onAddText = async (e: FormEvent) => {
    e.preventDefault();
    if (!snippet.trim()) return;
    await shelfAddText(snippet);
    setSnippet("");
  };

  const rowRef = useRef<HTMLDivElement>(null);
  const onWheel = (e: WheelEvent<HTMLDivElement>) => {
    const el = rowRef.current;
    if (!el || !rowCapturesWheel(el.scrollWidth, el.clientWidth)) return;
    // The row scrolls; the deck swipe in PeekView must not see this wheel.
    e.stopPropagation();
    if (Math.abs(e.deltaY) > Math.abs(e.deltaX)) el.scrollLeft += e.deltaY;
  };

  return (
    <div className="tray-deck">
      <div className="tray-head">
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
        <div className="shelf-body">
          <div className="shelf-row is-empty">
            {GHOSTS.map((g, i) => (
              <motion.div
                key={g.label}
                className="shelf-ghost"
                initial={{ opacity: 0, y: 6 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ delay: 0.04 * i, duration: 0.28, ease: [0.22, 1, 0.36, 1] }}
              >
                <motion.span
                  style={{ display: "grid" }}
                  initial={{ y: 0 }}
                  animate={{ y: [0, -5, 0] }}
                  transition={{ delay: 0.25 + 0.08 * i, duration: 0.5, ease: "easeOut" }}
                >
                  <Icon name={g.icon} size={18} />
                </motion.span>
                <span>{g.label}</span>
              </motion.div>
            ))}
            <div className="shelf-empty-copy">
              <strong>Полка пуста</strong>
              <span>Перетащите файл на остров или вставьте текст и ссылку выше</span>
            </div>
          </div>
          <TelegramTile target={null} />
        </div>
      ) : (
        <div className="shelf-body">
          <div className="shelf-row" ref={rowRef} onWheel={onWheel}>
            <AnimatePresence mode="popLayout">
              {shelf.map((item, i) => (
                <ShelfTile
                  key={item.id}
                  index={i}
                  fresh={!seenAtOpen.current.has(item.id)}
                  item={item}
                  selected={selected?.id === item.id}
                  onSelect={() => setSelectedId(item.id)}
                  onRemove={() => void shelfRemove(item.id)}
                />
              ))}
            </AnimatePresence>
          </div>
          <TelegramTile target={selected} />
        </div>
      )}
    </div>
  );
}
