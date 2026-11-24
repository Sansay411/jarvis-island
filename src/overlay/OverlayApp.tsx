import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { isTauri } from "../lib/ipc";
import { CardView } from "./cards/CardView";
import {
  clampToViewport,
  computeArrowGeometry,
  cssToPhysicalRect,
  physicalToCssPoint,
  physicalToCssRect,
  placeCaption,
  placeCard,
  rectsOverlap,
  type ViewportSize,
} from "./coords";
import type {
  CssRect,
  OverlayArrowItem,
  OverlayCaptionItem,
  OverlayCardItem,
  OverlayDrawPayload,
  OverlayItem,
  OverlayMonitor,
  OverlayRingItem,
  OverlaySnapshot,
  OverlayTone,
  Point2D,
  ScreenRect,
} from "./types";
import { liveItems, mergeItems, wrapItem, type ActiveItem } from "./items";
import "./overlay.css";

export interface OverlayQaProps {
  items: OverlayItem[];
  monitor: OverlayMonitor;
  pulseSeq?: number;
  reducedMotion?: boolean;
  onDismissCard?: (id: string) => void;
  onCancelGuide?: () => void;
}

export interface OverlayAppProps {
  qa?: OverlayQaProps;
}

const DEFAULT_MONITOR: OverlayMonitor = {
  originX: 0,
  originY: 0,
  width: 1920,
  height: 1080,
  scale: 1,
};

const CARD_DEFAULT_W = 316;
const CARD_DEFAULT_H = 160;
const PILL_EST_W = 340;
const PILL_EST_H = 92;

function toneColors(tone: OverlayTone | undefined): { ink: string; hi: string; glow: string } {
  switch (tone) {
    case "ok":
      return { ink: "#7cd992", hi: "#b8f0c5", glow: "rgba(124, 217, 146, 0.45)" };
    case "warn":
      return { ink: "#ffcf5a", hi: "#ffe6a3", glow: "rgba(255, 207, 90, 0.48)" };
    case "danger":
      return { ink: "#ff6b5a", hi: "#ffaba1", glow: "rgba(255, 107, 90, 0.48)" };
    default:
      return { ink: "#ffb23f", hi: "#ffd88f", glow: "rgba(255, 178, 63, 0.48)" };
  }
}

function rectsEqual(a: ScreenRect[], b: ScreenRect[]): boolean {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) {
    const ra = a[i];
    const rb = b[i];
    if (!ra || !rb || ra.x !== rb.x || ra.y !== rb.y || ra.width !== rb.width || ra.height !== rb.height) {
      return false;
    }
  }
  return true;
}

interface DraggableCardProps {
  card: OverlayCardItem;
  monitor: OverlayMonitor;
  viewport: ViewportSize;
  onClose: (id: string) => void;
  onMoved: () => void;
}

function DraggableCard({ card, monitor, viewport, onClose, onMoved }: DraggableCardProps) {
  const shellRef = useRef<HTMLDivElement | null>(null);
  const anchorCss = useMemo(
    () => (card.anchor ? physicalToCssRect(card.anchor, monitor) : undefined),
    [card.anchor, monitor],
  );
  const initial = useMemo(
    () => placeCard(anchorCss, viewport, CARD_DEFAULT_W, CARD_DEFAULT_H),
    [anchorCss, viewport.width, viewport.height],
  );
  const [pos, setPos] = useState<Point2D>(initial);
  const [dragging, setDragging] = useState(false);
  const dragOrigin = useRef<{ pointerX: number; pointerY: number; startX: number; startY: number } | null>(null);

  useEffect(() => {
    setPos(initial);
  }, [card.id, initial.x, initial.y]);

  useLayoutEffect(() => {
    onMoved();
  }, [pos.x, pos.y, onMoved]);

  const onPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    dragOrigin.current = { pointerX: e.clientX, pointerY: e.clientY, startX: pos.x, startY: pos.y };
    setDragging(true);
  };

  const onPointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    const o = dragOrigin.current;
    if (!o) return;
    const el = shellRef.current;
    const w = el ? el.offsetWidth : CARD_DEFAULT_W;
    const h = el ? el.offsetHeight : CARD_DEFAULT_H;
    const next = clampToViewport(
      o.startX + (e.clientX - o.pointerX),
      o.startY + (e.clientY - o.pointerY),
      w,
      h,
      viewport,
      12,
      12,
    );
    setPos(next);
  };

  const onPointerUp = (e: React.PointerEvent<HTMLDivElement>) => {
    if (!dragOrigin.current) return;
    dragOrigin.current = null;
    setDragging(false);
    try {
      e.currentTarget.releasePointerCapture(e.pointerId);
    } catch {
      // Ignore if pointer capture already ended.
    }
    onMoved();
  };

  return (
    <div
      ref={shellRef}
      className={`ov-card-shell${dragging ? " ov-card-shell--dragging" : ""}`}
      data-overlay-interactive=""
      style={{ left: `${pos.x}px`, top: `${pos.y}px` }}
    >
      <div
        className="ov-card-grip"
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={onPointerUp}
        title="Перетащить"
      >
        <span className="ov-card-grip-bar" aria-hidden="true" />
      </div>
      <CardView spec={card.spec} onClose={() => onClose(card.id)} />
    </div>
  );
}

export default function OverlayApp({ qa }: OverlayAppProps) {
  const rootRef = useRef<HTMLDivElement | null>(null);
  const lastReportedRef = useRef<ScreenRect[]>([]);
  const rafRef = useRef<number | null>(null);

  const [active, setActive] = useState<ActiveItem[]>(() =>
    qa ? qa.items.map((it) => wrapItem(it, Date.now())) : [],
  );
  const [monitor, setMonitor] = useState<OverlayMonitor>(() => qa?.monitor ?? DEFAULT_MONITOR);
  const [pulseSeq, setPulseSeq] = useState<number>(() => qa?.pulseSeq ?? 0);
  const [reducedMotion, setReducedMotion] = useState<boolean>(() => qa?.reducedMotion ?? false);
  const [viewport, setViewport] = useState<ViewportSize>(() => ({
    width: typeof window !== "undefined" ? window.innerWidth : 1920,
    height: typeof window !== "undefined" ? window.innerHeight : 1080,
  }));

  // Real sizes of captions and ring labels (`data-measure`), so they are
  // placed by what they are, not by a guess.
  const [sizes, setSizes] = useState<Record<string, { w: number; h: number }>>({});
  useLayoutEffect(() => {
    const root = rootRef.current;
    if (!root) return;
    const next: Record<string, { w: number; h: number }> = {};
    root.querySelectorAll<HTMLElement>("[data-measure]").forEach((el) => {
      const k = el.dataset.measure;
      if (k) next[k] = { w: el.offsetWidth, h: el.offsetHeight };
    });
    const keys = Object.keys(next);
    const same =
      keys.length === Object.keys(sizes).length &&
      keys.every((k) => sizes[k]?.w === next[k]?.w && sizes[k]?.h === next[k]?.h);
    if (!same) setSizes(next);
  });

  // Sync QA harness props directly.
  useEffect(() => {
    if (!qa) return;
    setActive(qa.items.map((it) => wrapItem(it, Date.now())));
    setMonitor(qa.monitor);
  }, [qa?.items, qa?.monitor]);

  useEffect(() => {
    if (qa?.pulseSeq !== undefined) setPulseSeq(qa.pulseSeq);
  }, [qa?.pulseSeq]);

  useEffect(() => {
    if (qa?.reducedMotion !== undefined) setReducedMotion(qa.reducedMotion);
  }, [qa?.reducedMotion]);

  // Track viewport size.
  useEffect(() => {
    const onResize = () => setViewport({ width: window.innerWidth, height: window.innerHeight });
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  // Connect to Tauri backend when running inside the real overlay window.
  useEffect(() => {
    if (qa || !isTauri()) return;
    let alive = true;
    const unlisteners: Array<() => void> = [];

    const applyDraw = (payload: OverlayDrawPayload) => {
      if (!alive) return;
      if (payload.monitor) setMonitor(payload.monitor);
      const now = Date.now();
      const incoming = payload.items.map((it) => wrapItem(it, now));
      setActive((prev) => mergeItems(prev, incoming, payload.replace));
    };

    const syncSettings = async () => {
      try {
        const view = await invoke<{ settings?: { reduceMotion?: boolean } }>("settings_get");
        if (alive && typeof view?.settings?.reduceMotion === "boolean") {
          setReducedMotion(view.settings.reduceMotion);
        }
      } catch {
        // Fallback to OS prefers-reduced-motion media query.
      }
    };

    void Promise.all([
      listen<OverlayDrawPayload>("overlay:draw", (e) => applyDraw(e.payload)),
      listen("overlay:clear", () => {
        if (alive) setActive([]);
      }),
      listen("overlay:pulse", () => {
        if (alive) setPulseSeq((n) => n + 1);
      }),
      listen("settings:changed", () => {
        void syncSettings();
      }),
    ]).then((fns) => {
      if (!alive) {
        fns.forEach((f) => f());
        return;
      }
      unlisteners.push(...fns);
      void invoke<OverlaySnapshot>("overlay_ready")
        .then((snap) => {
          if (!alive) return;
          setMonitor(snap.monitor);
          const now = Date.now();
          setActive(snap.items.map((it) => wrapItem(it, now)));
        })
        .catch(() => {});
      void syncSettings();
    });

    return () => {
      alive = false;
      unlisteners.forEach((f) => f());
    };
  }, [qa]);

  // Auto-hide timed items and tell Rust to hide the window once empty.
  useEffect(() => {
    if (qa) return;
    const timed = active.filter((a) => a.expiresAt !== null);
    if (timed.length === 0) return;

    const soonest = Math.min(...timed.map((a) => a.expiresAt as number));
    const delay = Math.max(40, soonest - Date.now() + 20);
    const timer = window.setTimeout(() => {
      const now = Date.now();
      setActive((prev) => {
        const remaining = liveItems(prev, now);
        if (remaining.length === 0 && prev.length > 0 && isTauri()) {
          void invoke("overlay_dismiss_item", { id: null, empty: true }).catch(() => {});
        }
        return remaining;
      });
    }, delay);

    return () => window.clearTimeout(timer);
  }, [active, qa]);

  // Report all [data-overlay-interactive] bounding rects to Rust for cursor hit-testing.
  const reportInteractiveRects = useCallback(() => {
    if (qa || !isTauri()) return;
    if (rafRef.current !== null) cancelAnimationFrame(rafRef.current);
    rafRef.current = requestAnimationFrame(() => {
      rafRef.current = null;
      const root = rootRef.current;
      if (!root) return;
      const nodes = root.querySelectorAll<HTMLElement>("[data-overlay-interactive]");
      const rects: ScreenRect[] = [];
      nodes.forEach((el) => {
        const r = el.getBoundingClientRect();
        if (r.width > 0 && r.height > 0) {
          rects.push(cssToPhysicalRect(r, monitor));
        }
      });
      if (!rectsEqual(rects, lastReportedRef.current)) {
        lastReportedRef.current = rects;
        void invoke("overlay_set_interactive_rects", { rects }).catch(() => {});
      }
    });
  }, [monitor, qa]);

  useEffect(() => {
    reportInteractiveRects();
    if (qa || !isTauri() || !rootRef.current) return;

    const mo = new MutationObserver(() => reportInteractiveRects());
    mo.observe(rootRef.current, { childList: true, subtree: true, attributes: true, attributeFilter: ["style", "class", "data-overlay-interactive"] });

    const ro = new ResizeObserver(() => reportInteractiveRects());
    ro.observe(rootRef.current);

    return () => {
      mo.disconnect();
      ro.disconnect();
      if (rafRef.current !== null) {
        cancelAnimationFrame(rafRef.current);
        rafRef.current = null;
      }
    };
  }, [active, monitor, viewport, reportInteractiveRects, qa]);

  const handleDismissCard = useCallback(
    (id: string) => {
      if (qa?.onDismissCard) {
        qa.onDismissCard(id);
        return;
      }
      setActive((prev) => {
        const next = prev.filter((a) => a.key !== id);
        if (isTauri()) {
          void invoke("overlay_dismiss_item", { id, empty: next.length === 0 }).catch(() => {});
        }
        return next;
      });
    },
    [qa],
  );

  const handleCancelGuide = useCallback(() => {
    if (qa?.onCancelGuide) {
      qa.onCancelGuide();
      return;
    }
    setActive([]);
    if (isTauri()) {
      void invoke("overlay_cancel_guide").catch(() => {});
    }
  }, [qa]);

  const rings = active.filter((a): a is ActiveItem & { item: OverlayRingItem } => a.item.kind === "ring");
  const arrows = active.filter((a): a is ActiveItem & { item: OverlayArrowItem } => a.item.kind === "arrow");
  const captions = active.filter((a): a is ActiveItem & { item: OverlayCaptionItem } => a.item.kind === "caption");
  const cards = active.filter((a): a is ActiveItem & { item: OverlayCardItem } => a.item.kind === "card");

  const primaryRingCss: CssRect | undefined = rings[0]
    ? physicalToCssRect(
        { x: rings[0].item.x, y: rings[0].item.y, w: rings[0].item.w, h: rings[0].item.h },
        monitor,
      )
    : undefined;

  const sizeOf = (k: string, w: number, h: number) => sizes[k] ?? { w, h };

  const captionBoxes = new Map<string, CssRect>();
  for (const { key, item } of captions) {
    const explicit =
      typeof item.x === "number" && typeof item.y === "number"
        ? physicalToCssPoint({ x: item.x, y: item.y }, monitor)
        : undefined;
    const targetCss = item.targetRect ? physicalToCssRect(item.targetRect, monitor) : primaryRingCss;
    const size = sizeOf(`cap-${key}`, PILL_EST_W, PILL_EST_H);
    const pos = placeCaption(explicit, targetCss, viewport, size.w, size.h);
    captionBoxes.set(key, { x: pos.x, y: pos.y, w: size.w, h: size.h });
  }

  const labelBoxes = new Map<string, CssRect>();
  for (const { key, item } of rings) {
    if (!item.label || !item.label.trim()) continue;
    const css = physicalToCssRect({ x: item.x, y: item.y, w: item.w, h: item.h }, monitor);
    const size = sizeOf(`lbl-${key}`, 160, 28);
    const cx = css.x + css.w / 2 - size.w / 2;
    const tries = [
      { x: cx, y: css.y - size.h - 14 },
      { x: cx, y: css.y + css.h + 14 },
      { x: css.x + css.w + 14, y: css.y + css.h / 2 - size.h / 2 },
      { x: css.x - size.w - 14, y: css.y + css.h / 2 - size.h / 2 },
    ].map((p) => clampToViewport(p.x, p.y, size.w, size.h, viewport, 12, 12));
    const fits = (p: Point2D) => {
      const box = { x: p.x, y: p.y, w: size.w, h: size.h };
      return !rectsOverlap(box, css, 4) && ![...captionBoxes.values()].some((c) => rectsOverlap(box, c, 6));
    };
    const pos = tries.find(fits) ?? tries[0]!;
    labelBoxes.set(key, { x: pos.x, y: pos.y, w: size.w, h: size.h });
  }
  const arrowAvoid = [...captionBoxes.values(), ...labelBoxes.values()];

  return (
    <div
      ref={rootRef}
      className="ov-root"
      data-reduced-motion={reducedMotion ? "true" : "false"}
      aria-live="polite"
    >
      {/* Vector layer: rings and curved arrows */}
      <svg className="ov-svg" shapeRendering="geometricPrecision">
        <defs>
          <filter id="ov-amber-blur" x="-40%" y="-40%" width="180%" height="180%">
            <feGaussianBlur stdDeviation="5" />
          </filter>
        </defs>

        {rings.map(({ key, item }) => {
          const css = physicalToCssRect({ x: item.x, y: item.y, w: item.w, h: item.h }, monitor);
          const pad = 5;
          const x = css.x - pad;
          const y = css.y - pad;
          const w = Math.max(16, css.w + pad * 2);
          const h = Math.max(16, css.h + pad * 2);
          const rx = Math.min(14, Math.max(8, Math.min(w, h) * 0.28));
          const c = toneColors(item.tone);

          return (
            <g key={key}>
              <rect
                className="ov-ring-halo"
                x={x}
                y={y}
                width={w}
                height={h}
                rx={rx}
                stroke={c.glow}
                filter="url(#ov-amber-blur)"
              />
              <rect
                className="ov-ring-underlay"
                x={x}
                y={y}
                width={w}
                height={h}
                rx={rx}
                strokeWidth={5}
              />
              <rect
                className="ov-ring-stroke"
                x={x}
                y={y}
                width={w}
                height={h}
                rx={rx}
                stroke={c.ink}
                strokeWidth={2.4}
                pathLength={1}
              />
              <rect
                className="ov-ring-inner"
                x={x + 1.5}
                y={y + 1.5}
                width={Math.max(4, w - 3)}
                height={Math.max(4, h - 3)}
                rx={Math.max(4, rx - 1.5)}
                stroke={c.hi}
                strokeWidth={1}
              />
              {pulseSeq > 0 ? (
                <rect
                  key={`${key}-pulse-${pulseSeq}`}
                  className="ov-ring-ripple"
                  x={x}
                  y={y}
                  width={w}
                  height={h}
                  rx={rx}
                  stroke={c.hi}
                />
              ) : null}
            </g>
          );
        })}

        {arrows.map(({ key, item }) => {
          const toCss = physicalToCssPoint(item.to, monitor);
          const fromCss = item.from ? physicalToCssPoint(item.from, monitor) : undefined;
          const targetCss = item.targetRect
            ? physicalToCssRect(item.targetRect, monitor)
            : primaryRingCss;
          const curve = computeArrowGeometry(toCss, fromCss, targetCss, viewport, arrowAvoid);
          const c = toneColors(item.tone);
          const d = `M ${curve.start.x.toFixed(1)} ${curve.start.y.toFixed(1)} Q ${curve.control.x.toFixed(1)} ${curve.control.y.toFixed(1)} ${curve.end.x.toFixed(1)} ${curve.end.y.toFixed(1)}`;

          return (
            <g key={key} className="ov-arrow-group">
              <path d={d} className="ov-arrow-underlay" />
              <path
                d={d}
                className="ov-arrow-path"
                stroke={c.ink}
                pathLength={1}
                style={{ filter: `drop-shadow(0 0 6px ${c.glow})` }}
              />
              <g transform={`translate(${curve.end.x.toFixed(1)} ${curve.end.y.toFixed(1)}) rotate(${curve.angleDeg.toFixed(1)})`}>
                <path d="M -11 -6.5 L 0 0 L -11 6.5" className="ov-arrow-underlay" />
                <path d="M -11 -6.5 L 0 0 L -11 6.5" className="ov-arrow-head" stroke={c.hi} />
              </g>
            </g>
          );
        })}
      </svg>

      {/* Ring floating labels */}
      {rings.map(({ key, item }) => {
        const box = labelBoxes.get(key);
        if (!box || !item.label) return null;
        const c = toneColors(item.tone);
        return (
          <div
            key={`${key}-lbl`}
            className="ov-ring-label"
            data-measure={`lbl-${key}`}
            style={{ left: `${box.x}px`, top: `${box.y}px` }}
          >
            <span className="ov-ring-label-dot" style={{ background: c.ink, color: c.ink }} />
            <span>{item.label}</span>
          </div>
        );
      })}

      {/* Caption pills */}
      {captions.map(({ key, item }) => {
        const pos = captionBoxes.get(key) ?? { x: 0, y: 0 };
        const hasSteps = typeof item.step === "number" && typeof item.total === "number" && item.total > 0;
        const stepCount = hasSteps ? Math.min(item.total as number, 10) : 0;

        return (
          <div
            key={key}
            className={`ov-caption${item.tone === "ok" ? " ov-caption--ok" : ""}`}
            data-measure={`cap-${key}`}
            style={{ left: `${pos.x}px`, top: `${pos.y}px` }}
          >
            {hasSteps || item.badge || item.closable ? (
              <div className="ov-caption-top">
                <div className="ov-caption-meta">
                  {item.badge ? (
                    <span className="ov-caption-badge">{item.badge}</span>
                  ) : null}
                  {hasSteps ? (
                    <>
                      <span className="ov-step-dots" aria-hidden="true">
                        {Array.from({ length: stepCount }, (_, idx) => {
                          const n = idx + 1;
                          const cls =
                            n < (item.step as number)
                              ? "ov-step-dot ov-step-dot--done"
                              : n === item.step
                                ? "ov-step-dot ov-step-dot--active"
                                : "ov-step-dot";
                          return <span key={n} className={cls} />;
                        })}
                      </span>
                      <span className="ov-caption-step">
                        Шаг {item.step} из {item.total}
                      </span>
                    </>
                  ) : null}
                </div>
                {item.closable ? (
                  <button
                    type="button"
                    className="ov-caption-close"
                    data-overlay-interactive=""
                    onClick={handleCancelGuide}
                    aria-label="Остановить подсказку"
                    title="Остановить (Esc)"
                  >
                    <svg width="11" height="11" viewBox="0 0 12 12" fill="none" aria-hidden="true">
                      <path d="M2.5 2.5L9.5 9.5M9.5 2.5L2.5 9.5" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
                    </svg>
                  </button>
                ) : null}
              </div>
            ) : null}

            <p className="ov-caption-text">{item.text}</p>

            {item.hint ? (
              <div className="ov-caption-hint">
                <span>{item.hint}</span>
                <span>·</span>
                <kbd className="ov-kbd">Esc</kbd>
                <span>отмена</span>
              </div>
            ) : null}
          </div>
        );
      })}

      {/* Draggable interactive cards */}
      {cards.map(({ key, item }) => (
        <DraggableCard
          key={key}
          card={item}
          monitor={monitor}
          viewport={viewport}
          onClose={handleDismissCard}
          onMoved={reportInteractiveRects}
        />
      ))}
    </div>
  );
}
