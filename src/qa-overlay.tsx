// Visual QA harness for the JARVIS screen overlay (`qa/overlay.html`).
// Renders the real `OverlayApp` over a realistic fake desktop application
// window. Supports `?scene=ring|guide|arrow|card` (plus `done`), simulated DPI
// scales (100–200%), reduced-motion toggle, missed-click pulse, interactive
// guide progression and card dragging/dismissal.

import { StrictMode, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import "@fontsource-variable/onest";
import "@fontsource-variable/jetbrains-mono";
import "./styles/tokens.css";
import "./styles/base.css";
import OverlayApp from "./overlay/OverlayApp";
import type { OverlayItem, OverlayMonitor } from "./overlay/types";

type SceneId = "ring" | "guide" | "arrow" | "card" | "done";

interface Box {
  x: number;
  y: number;
  w: number;
  h: number;
}

function parseScene(): SceneId {
  const s = new URLSearchParams(location.search).get("scene");
  if (s === "ring" || s === "guide" || s === "arrow" || s === "card" || s === "done") return s;
  return "guide";
}

function QaOverlayHarness() {
  const [scene, setScene] = useState<SceneId>(parseScene);
  const [dpi, setDpi] = useState<number>(1.25);
  const [reducedMotion, setReducedMotion] = useState<boolean>(false);
  const [pulseSeq, setPulseSeq] = useState<number>(0);
  const [guideIdx, setGuideIdx] = useState<number>(1);
  const [cardClosed, setCardClosed] = useState<boolean>(false);

  const exportBtnRef = useRef<HTMLButtonElement | null>(null);
  const settingsBtnRef = useRef<HTMLButtonElement | null>(null);
  const syncToggleRef = useRef<HTMLButtonElement | null>(null);
  const shareBtnRef = useRef<HTMLButtonElement | null>(null);

  const [boxes, setBoxes] = useState<{ export: Box; settings: Box; sync: Box; share: Box }>({
    export: { x: 860, y: 150, w: 124, h: 34 },
    settings: { x: 240, y: 400, w: 180, h: 36 },
    sync: { x: 760, y: 310, w: 54, h: 28 },
    share: { x: 730, y: 150, w: 112, h: 34 },
  });

  const measure = () => {
    const read = (el: HTMLElement | null, fallback: Box): Box => {
      if (!el) return fallback;
      const r = el.getBoundingClientRect();
      return { x: r.left, y: r.top, w: r.width, h: r.height };
    };
    setBoxes((prev) => ({
      export: read(exportBtnRef.current, prev.export),
      settings: read(settingsBtnRef.current, prev.settings),
      sync: read(syncToggleRef.current, prev.sync),
      share: read(shareBtnRef.current, prev.share),
    }));
  };

  useLayoutEffect(() => {
    measure();
    // Web fonts and late layout move the fake controls: measure again.
    void document.fonts?.ready.then(measure);
    document.fonts?.addEventListener("loadingdone", measure);
    const ro = new ResizeObserver(measure);
    for (const el of [exportBtnRef, settingsBtnRef, syncToggleRef, shareBtnRef]) {
      const parent = el.current?.parentElement;
      if (parent) ro.observe(parent);
    }
    window.addEventListener("resize", measure);
    return () => {
      ro.disconnect();
      document.fonts?.removeEventListener("loadingdone", measure);
      window.removeEventListener("resize", measure);
    };
  }, []);

  useEffect(() => {
    const url = new URL(location.href);
    url.searchParams.set("scene", scene);
    history.replaceState(null, "", url.toString());
    if (scene === "card") setCardClosed(false);
    if (scene === "guide") setGuideIdx(1);
  }, [scene]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && scene === "guide") {
        setScene("ring");
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [scene]);

  // Simulate physical monitor coordinates at the chosen DPI scale.
  const monitor: OverlayMonitor = useMemo(
    () => ({
      originX: 0,
      originY: 0,
      width: Math.round(window.innerWidth * dpi),
      height: Math.round(window.innerHeight * dpi),
      scale: dpi,
    }),
    [dpi],
  );

  const toPhys = (b: Box) => ({
    x: Math.round(b.x * dpi),
    y: Math.round(b.y * dpi),
    w: Math.round(b.w * dpi),
    h: Math.round(b.h * dpi),
  });

  const items: OverlayItem[] = useMemo(() => {
    if (scene === "ring") {
      const r = toPhys(boxes.export);
      return [
        { kind: "ring", id: "qa-ring", ...r, label: "Экспорт в PDF", tone: "amber", persistent: true },
      ];
    }

    if (scene === "arrow") {
      const r = toPhys(boxes.share);
      return [
        { kind: "ring", id: "qa-arrow-ring", ...r, label: "Поделиться", tone: "amber", persistent: true },
        {
          kind: "arrow",
          id: "qa-arrow",
          to: { x: r.x + Math.round(r.w / 2), y: r.y + Math.round(r.h / 2) },
          targetRect: r,
          tone: "amber",
          persistent: true,
        },
        {
          kind: "caption",
          id: "qa-arrow-cap",
          text: "Кнопка «Поделиться» открывает доступ по ссылке.",
          targetRect: r,
          persistent: true,
        },
      ];
    }

    if (scene === "card") {
      if (cardClosed) return [];
      const anchor = toPhys(boxes.sync);
      return [
        { kind: "ring", id: "qa-card-ring", ...anchor, tone: "amber", persistent: true },
        {
          kind: "card",
          id: "qa-card-1",
          anchor,
          spec: {
            title: "Автосинхронизация",
            blocks: [
              {
                kind: "text",
                text: "Сохраняет черновики в облаке каждые 30 секунд. Карточку можно перетащить за верхнюю полоску или закрыть крестиком.",
              },
            ],
          },
        },
      ];
    }

    if (scene === "done") {
      return [
        {
          kind: "caption",
          id: "qa-done",
          text: "Автосинхронизация включена. Все шаги выполнены.",
          badge: "Готово",
          step: 3,
          total: 3,
          tone: "ok",
          persistent: true,
        },
      ];
    }

    // scene === "guide"
    const steps: Array<{ target: Box; text: string; label: string }> = [
      { target: boxes.settings, text: "Сначала откройте раздел «Параметры» в левом меню.", label: "Параметры" },
      { target: boxes.sync, text: "Теперь включите переключатель «Автосинхронизация».", label: "Автосинхронизация" },
      { target: boxes.export, text: "Нажмите «Экспорт PDF», чтобы сохранить готовый файл.", label: "Экспорт PDF" },
    ];
    const current = steps[Math.min(guideIdx - 1, steps.length - 1)] ?? steps[0]!;
    const r = toPhys(current.target);
    return [
      { kind: "ring", id: `qa-g-ring-${guideIdx}`, ...r, label: current.label, tone: "amber", persistent: true },
      {
        kind: "arrow",
        id: `qa-g-arrow-${guideIdx}`,
        to: { x: r.x + Math.round(r.w / 2), y: r.y + Math.round(r.h / 2) },
        targetRect: r,
        tone: "amber",
        persistent: true,
      },
      {
        kind: "caption",
        id: `qa-g-cap-${guideIdx}`,
        text: current.text,
        step: guideIdx,
        total: steps.length,
        targetRect: r,
        hint: "Нажмите в подсвеченную область",
        persistent: true,
        closable: true,
      },
    ];
  }, [scene, boxes, dpi, guideIdx, cardClosed]);

  const handleTargetClick = (which: "settings" | "sync" | "export") => {
    if (scene !== "guide") return;
    const expected = guideIdx === 1 ? "settings" : guideIdx === 2 ? "sync" : "export";
    if (which === expected) {
      if (guideIdx >= 3) setScene("done");
      else setGuideIdx((n) => n + 1);
    } else {
      setPulseSeq((n) => n + 1);
    }
  };

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        background:
          "radial-gradient(1200px 700px at 18% 12%, #1e2433 0%, #10131a 55%, #090b10 100%)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        padding: "64px 48px 84px",
      }}
      onClick={(e) => {
        if (scene === "guide" && e.target === e.currentTarget) {
          setPulseSeq((n) => n + 1);
        }
      }}
    >
      {/* Fake top Dynamic Island notch for safe-zone visual check */}
      <div
        aria-hidden="true"
        style={{
          position: "fixed",
          top: 10,
          left: "50%",
          transform: "translateX(-50%)",
          width: 146,
          height: 32,
          borderRadius: 999,
          background: "#08080a",
          border: "1px solid rgba(255,255,255,0.12)",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          gap: 10,
          boxShadow: "0 10px 28px rgba(0,0,0,0.6)",
          zIndex: 1,
        }}
      >
        <span style={{ width: 9, height: 9, borderRadius: "50%", background: "#ffb23f", boxShadow: "0 0 10px rgba(255,178,63,0.6)" }} />
        <span style={{ width: 9, height: 9, borderRadius: "50%", background: "#ffb23f", boxShadow: "0 0 10px rgba(255,178,63,0.6)" }} />
      </div>

      {/* Simulated desktop application screenshot */}
      <div
        style={{
          width: "min(980px, 100%)",
          height: "min(560px, 100%)",
          borderRadius: 14,
          background: "#f6f7f9",
          color: "#15171c",
          boxShadow: "0 28px 80px rgba(0,0,0,0.55), 0 0 0 1px rgba(255,255,255,0.12)",
          display: "grid",
          gridTemplateColumns: "220px 1fr",
          gridTemplateRows: "54px 1fr",
          overflow: "hidden",
        }}
      >
        {/* App top toolbar */}
        <div
          style={{
            gridColumn: "1 / -1",
            background: "#ffffff",
            borderBottom: "1px solid #e4e6eb",
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            padding: "0 18px",
          }}
        >
          <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
            <span style={{ width: 10, height: 10, borderRadius: "50%", background: "#d4d8e1" }} />
            <span style={{ fontWeight: 650, fontSize: 13 }}>Квартальный отчёт — Документы</span>
          </div>
          <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
            <button
              ref={shareBtnRef}
              type="button"
              onClick={() => setPulseSeq((n) => n + 1)}
              style={{
                height: 32,
                padding: "0 14px",
                borderRadius: 8,
                background: "#eef0f4",
                color: "#1c2028",
                fontWeight: 560,
                fontSize: 12,
              }}
            >
              Поделиться
            </button>
            <button
              ref={exportBtnRef}
              type="button"
              onClick={() => handleTargetClick("export")}
              style={{
                height: 32,
                padding: "0 14px",
                borderRadius: 8,
                background: "#16181f",
                color: "#ffffff",
                fontWeight: 560,
                fontSize: 12,
              }}
            >
              Экспорт PDF
            </button>
          </div>
        </div>

        {/* Left sidebar */}
        <div
          style={{
            background: "#f0f2f6",
            borderRight: "1px solid #e4e6eb",
            padding: 14,
            display: "flex",
            flexDirection: "column",
            gap: 6,
          }}
        >
          {["Обзор", "Таблицы", "Диаграммы", "История версий"].map((item) => (
            <div
              key={item}
              style={{
                padding: "8px 10px",
                borderRadius: 8,
                fontSize: 13,
                color: "#4a5160",
              }}
            >
              {item}
            </div>
          ))}
          <button
            ref={settingsBtnRef}
            type="button"
            onClick={() => handleTargetClick("settings")}
            style={{
              textAlign: "left",
              padding: "9px 10px",
              borderRadius: 8,
              fontSize: 13,
              fontWeight: 600,
              color: "#15171c",
              background: "#e3e7ef",
              marginTop: 12,
            }}
          >
            Параметры
          </button>
        </div>

        {/* Main settings pane */}
        <div style={{ padding: "24px 28px", display: "flex", flexDirection: "column", gap: 18 }}>
          <div>
            <div style={{ fontSize: 16, fontWeight: 650 }}>Параметры документа</div>
            <div style={{ fontSize: 12, color: "#687082", marginTop: 2 }}>
              Светлая поверхность приложения позволяет проверить контраст янтарной подсветки.
            </div>
          </div>

          <div
            style={{
              background: "#ffffff",
              border: "1px solid #e4e6eb",
              borderRadius: 12,
              padding: "16px 18px",
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
            }}
          >
            <div>
              <div style={{ fontSize: 13, fontWeight: 600 }}>Автосинхронизация</div>
              <div style={{ fontSize: 12, color: "#687082" }}>Сохранять изменения автоматически</div>
            </div>
            <button
              ref={syncToggleRef}
              type="button"
              onClick={() => handleTargetClick("sync")}
              aria-label="Автосинхронизация"
              style={{
                width: 48,
                height: 26,
                borderRadius: 999,
                background: guideIdx >= 3 ? "#16181f" : "#cfd5e2",
                padding: 3,
                display: "flex",
                justifyContent: guideIdx >= 3 ? "flex-end" : "flex-start",
                alignItems: "center",
              }}
            >
              <span style={{ width: 20, height: 20, borderRadius: "50%", background: "#ffffff", boxShadow: "0 1px 3px rgba(0,0,0,0.25)" }} />
            </button>
          </div>

          <div
            style={{
              background: "#ffffff",
              border: "1px solid #e4e6eb",
              borderRadius: 12,
              padding: "16px 18px",
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
              opacity: 0.72,
            }}
          >
            <div>
              <div style={{ fontSize: 13, fontWeight: 600 }}>Сжатие изображений</div>
              <div style={{ fontSize: 12, color: "#687082" }}>Уменьшать вложения перед экспортом</div>
            </div>
            <span style={{ fontSize: 12, color: "#687082" }}>Вкл</span>
          </div>
        </div>
      </div>

      {/* Real OverlayApp mounted on top */}
      <OverlayApp
        qa={{
          items,
          monitor,
          pulseSeq,
          reducedMotion,
          onDismissCard: () => setCardClosed(true),
          onCancelGuide: () => setScene("ring"),
        }}
      />

      {/* Bottom QA switcher bar */}
      <div
        style={{
          position: "fixed",
          bottom: 16,
          left: "50%",
          transform: "translateX(-50%)",
          display: "flex",
          alignItems: "center",
          gap: 6,
          padding: "6px 8px",
          borderRadius: 12,
          background: "rgba(10, 10, 13, 0.92)",
          border: "1px solid rgba(255,255,255,0.14)",
          backdropFilter: "blur(12px)",
          zIndex: 100,
        }}
      >
        {(["ring", "guide", "arrow", "card", "done"] as SceneId[]).map((s) => (
          <button
            key={s}
            type="button"
            onClick={() => setScene(s)}
            className={`btn ${scene === s ? "btn-solid" : "btn-quiet"}`}
            style={{ height: 26, padding: "0 10px", fontSize: 11 }}
          >
            {s}
          </button>
        ))}

        <span style={{ width: 1, height: 18, background: "rgba(255,255,255,0.12)", margin: "0 4px" }} />

        <button
          type="button"
          className="btn btn-quiet"
          style={{ height: 26, padding: "0 10px", fontSize: 11 }}
          onClick={() => setPulseSeq((n) => n + 1)}
        >
          Пульс
        </button>

        <button
          type="button"
          className="btn btn-quiet"
          style={{ height: 26, padding: "0 10px", fontSize: 11 }}
          onClick={() => setDpi((d) => (d === 1 ? 1.25 : d === 1.25 ? 1.5 : d === 1.5 ? 2 : 1))}
        >
          DPI {Math.round(dpi * 100)}%
        </button>

        <button
          type="button"
          className={`btn ${reducedMotion ? "btn-solid" : "btn-quiet"}`}
          style={{ height: 26, padding: "0 10px", fontSize: 11 }}
          onClick={() => setReducedMotion((v) => !v)}
        >
          Reduce motion
        </button>
      </div>
    </div>
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <QaOverlayHarness />
  </StrictMode>,
);
