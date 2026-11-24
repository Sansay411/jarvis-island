import { useCallback, useEffect, useId, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { errorMessage, isTauri } from "../lib/ipc";
import {
  hitTestWindow,
  isClickGesture,
  normalizeLogicalRect,
  physicalSizeOf,
  sizeLabelPosition,
  type LogicalRect,
  type PickerState,
  type PickerWindow,
} from "./selection";
import "./areapick.css";

export interface AreaPickProps {
  /** Optional mock state for the standalone browser QA page (`qa/areapick.html`). */
  qaState?: PickerState;
  onQaConfirmRect?: (rect: LogicalRect, phys: { width: number; height: number }) => void;
  onQaConfirmWindow?: (win: PickerWindow) => void;
  onQaCancel?: () => void;
}

export default function AreaPickApp({ qaState, onQaConfirmRect, onQaConfirmWindow, onQaCancel }: AreaPickProps = {}) {
  const maskId = useId();
  const rootRef = useRef<HTMLDivElement>(null);
  const [state, setState] = useState<PickerState | null>(qaState ?? null);
  const [drag, setDrag] = useState<{ x1: number; y1: number; x2: number; y2: number } | null>(null);
  const [cursor, setCursor] = useState<{ x: number; y: number } | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const pointerIdRef = useRef<number | null>(null);

  useEffect(() => {
    if (qaState) {
      setState(qaState);
      return;
    }
    if (!isTauri()) return;
    void invoke<PickerState | null>("area_picker_state")
      .then((s) => {
        if (s) setState(s);
      })
      .catch(() => {});
  }, [qaState]);

  const scale = state?.scale ?? window.devicePixelRatio ?? 1;
  const viewW = state?.logicalWidth ?? window.innerWidth;
  const viewH = state?.logicalHeight ?? window.innerHeight;

  const selection = useMemo<LogicalRect | null>(() => {
    if (!drag) return null;
    if (isClickGesture(drag.x1, drag.y1, drag.x2, drag.y2)) return null;
    return normalizeLogicalRect(drag.x1, drag.y1, drag.x2, drag.y2, viewW, viewH);
  }, [drag, viewW, viewH]);

  const hoveredWindow = useMemo<PickerWindow | null>(() => {
    if (selection || !cursor || !state?.windows.length) return null;
    return hitTestWindow(cursor.x, cursor.y, state.windows);
  }, [selection, cursor, state]);

  const cancel = useCallback(() => {
    if (submitting) return;
    setDrag(null);
    if (onQaCancel) {
      onQaCancel();
      return;
    }
    if (isTauri()) {
      void invoke("area_cancel");
    }
  }, [submitting, onQaCancel]);

  const confirmRect = useCallback(
    async (x1: number, y1: number, x2: number, y2: number) => {
      if (submitting) return;
      const rect = normalizeLogicalRect(x1, y1, x2, y2, viewW, viewH);
      const phys = physicalSizeOf(rect, scale);
      if (phys.width < 4 || phys.height < 4) {
        setDrag(null);
        return;
      }
      if (onQaConfirmRect) {
        onQaConfirmRect(rect, phys);
        setDrag(null);
        return;
      }
      if (!isTauri()) return;
      setSubmitting(true);
      setError(null);
      try {
        await invoke("area_confirm_rect", { x1, y1, x2, y2 });
      } catch (e) {
        setSubmitting(false);
        setError(errorMessage(e));
      }
    },
    [submitting, viewW, viewH, scale, onQaConfirmRect],
  );

  const confirmClick = useCallback(
    async (x: number, y: number, win: PickerWindow | null) => {
      if (submitting) return;
      if (onQaConfirmWindow) {
        if (win) onQaConfirmWindow(win);
        setDrag(null);
        return;
      }
      if (!isTauri()) return;
      setSubmitting(true);
      setError(null);
      try {
        await invoke("area_confirm_click", { x, y, hwnd: win?.hwnd ?? null });
      } catch (e) {
        setSubmitting(false);
        setError(errorMessage(e));
      }
    },
    [submitting, onQaConfirmWindow],
  );

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        cancel();
      } else if (e.key === "Enter") {
        e.preventDefault();
        if (drag && !isClickGesture(drag.x1, drag.y1, drag.x2, drag.y2)) {
          void confirmRect(drag.x1, drag.y1, drag.x2, drag.y2);
        } else if (cursor) {
          void confirmClick(cursor.x, cursor.y, hoveredWindow);
        }
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [cancel, confirmRect, confirmClick, drag, cursor, hoveredWindow]);

  const localPoint = (e: ReactPointerEvent<HTMLDivElement>) => {
    const box = rootRef.current?.getBoundingClientRect();
    return {
      x: e.clientX - (box?.left ?? 0),
      y: e.clientY - (box?.top ?? 0),
    };
  };

  const onPointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (submitting) return;
    if (e.button === 2) {
      e.preventDefault();
      cancel();
      return;
    }
    if (e.button !== 0) return;
    const p = localPoint(e);
    pointerIdRef.current = e.pointerId;
    e.currentTarget.setPointerCapture(e.pointerId);
    setCursor(p);
    setDrag({ x1: p.x, y1: p.y, x2: p.x, y2: p.y });
    setError(null);
  };

  const onPointerMove = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (submitting) return;
    const p = localPoint(e);
    setCursor(p);
    if (pointerIdRef.current === e.pointerId) {
      setDrag((prev) => (prev ? { ...prev, x2: p.x, y2: p.y } : prev));
    }
  };

  const onPointerUp = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (pointerIdRef.current !== e.pointerId) return;
    pointerIdRef.current = null;
    if (e.currentTarget.hasPointerCapture(e.pointerId)) {
      e.currentTarget.releasePointerCapture(e.pointerId);
    }
    if (!drag || submitting) return;
    const p = localPoint(e);
    if (isClickGesture(drag.x1, drag.y1, p.x, p.y)) {
      const targetWin = state?.windows.length ? hitTestWindow(p.x, p.y, state.windows) : null;
      setDrag(null);
      void confirmClick(p.x, p.y, targetWin);
    } else {
      void confirmRect(drag.x1, drag.y1, p.x, p.y);
    }
  };

  // Cutout rect for the SVG mask: either the active drag selection or the
  // hovered window so the target stands out crisply against the 35% dim.
  const cutout: LogicalRect | null = selection
    ? selection
    : hoveredWindow
      ? {
          x: hoveredWindow.localX,
          y: hoveredWindow.localY,
          width: hoveredWindow.localWidth,
          height: hoveredWindow.localHeight,
        }
      : null;

  const physSize = selection ? physicalSizeOf(selection, scale) : null;
  const pillPos = selection ? sizeLabelPosition(selection, viewW, viewH) : null;

  return (
    <div
      ref={rootRef}
      className="areapick-root"
      role="application"
      aria-label="Выбор области экрана"
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onContextMenu={(e) => {
        e.preventDefault();
        cancel();
      }}
    >
      <svg className="areapick-backdrop" aria-hidden="true">
        <defs>
          <mask id={maskId}>
            <rect x="0" y="0" width="100%" height="100%" fill="white" />
            {cutout && (
              <rect
                x={cutout.x}
                y={cutout.y}
                width={cutout.width}
                height={cutout.height}
                rx={selection ? 4 : 8}
                fill={selection ? "black" : "rgba(0,0,0,0.45)"}
              />
            )}
          </mask>
        </defs>
        <rect x="0" y="0" width="100%" height="100%" fill="rgba(4, 4, 6, 0.36)" mask={`url(#${maskId})`} />
      </svg>

      <div className="areapick-hint" aria-live="polite">
        <span className="areapick-hint-dot" />
        <span>Выделите область</span>
        <span className="areapick-hint-sep">·</span>
        <span className="areapick-hint-key">клик — окно</span>
        <span className="areapick-hint-sep">·</span>
        <span className="areapick-hint-key">Esc — отмена</span>
      </div>

      {hoveredWindow && !selection && (
        <div
          className="areapick-window-hover"
          style={{
            left: hoveredWindow.localX,
            top: hoveredWindow.localY,
            width: hoveredWindow.localWidth,
            height: hoveredWindow.localHeight,
          }}
        >
          <div className="areapick-window-tag">
            <span>{hoveredWindow.title || hoveredWindow.process}</span>
            <span className="areapick-window-tag-dim">
              {hoveredWindow.rect.width}×{hoveredWindow.rect.height}
            </span>
          </div>
        </div>
      )}

      {selection && (
        <div
          className="areapick-selection"
          style={{
            left: selection.x,
            top: selection.y,
            width: selection.width,
            height: selection.height,
          }}
        >
          <span className="areapick-handle is-tl" />
          <span className="areapick-handle is-tr" />
          <span className="areapick-handle is-bl" />
          <span className="areapick-handle is-br" />
        </div>
      )}

      {selection && physSize && pillPos && (
        <div className="areapick-size-pill" style={{ left: pillPos.left, top: pillPos.top }}>
          {physSize.width} × {physSize.height}
        </div>
      )}

      {error && (
        <div className="areapick-error" role="alert">
          {error}
        </div>
      )}
    </div>
  );
}
