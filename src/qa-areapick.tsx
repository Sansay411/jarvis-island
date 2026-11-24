// Visual QA for the fullscreen area picker (`qa/areapick.html`): renders the
// real AreaPickApp over a synthetic desktop screenshot with mock windows so
// drag selection, corner handles, size readout, single-click window pick and
// Esc/right-click cancel can be exercised in any browser.

import { StrictMode, useMemo, useState } from "react";
import { createRoot } from "react-dom/client";
import "@fontsource-variable/onest";
import "@fontsource-variable/jetbrains-mono";
import "./styles/tokens.css";
import "./styles/base.css";
import AreaPickApp from "./areapick/AreaPickApp";
import type { PickerState } from "./areapick/selection";

function QaDesktop() {
  const [toast, setToast] = useState<string | null>(null);
  const scale = window.devicePixelRatio || 1.5;

  const qaState = useMemo<PickerState>(() => {
    const w = window.innerWidth;
    const h = window.innerHeight;
    const code = { x: 64, y: 74, w: Math.min(680, w - 420), h: Math.min(520, h - 140) };
    const term = { x: Math.max(code.x + code.w - 120, w - 460), y: 180, w: 400, h: 320 };
    return {
      monitorBounds: { x: 0, y: 0, width: Math.round(w * scale), height: Math.round(h * scale) },
      scale,
      logicalWidth: w,
      logicalHeight: h,
      windows: [
        {
          hwnd: 201,
          title: "Windows Terminal — cargo test",
          process: "WindowsTerminal.exe",
          rect: {
            x: Math.round(term.x * scale),
            y: Math.round(term.y * scale),
            width: Math.round(term.w * scale),
            height: Math.round(term.h * scale),
          },
          localX: term.x,
          localY: term.y,
          localWidth: term.w,
          localHeight: term.h,
        },
        {
          hwnd: 101,
          title: "area.rs — JARVIS — Visual Studio Code",
          process: "Code.exe",
          rect: {
            x: Math.round(code.x * scale),
            y: Math.round(code.y * scale),
            width: Math.round(code.w * scale),
            height: Math.round(code.h * scale),
          },
          localX: code.x,
          localY: code.y,
          localWidth: code.w,
          localHeight: code.h,
        },
      ],
    };
  }, [scale]);

  const codeWin = qaState.windows[1]!;
  const termWin = qaState.windows[0]!;

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        background:
          "radial-gradient(900px 600px at 18% 15%, #1d2638 0%, #10141f 48%, #0a0c12 100%)",
        color: "#e6e8ef",
        fontFamily: "var(--font-ui)",
      }}
    >
      {/* Simulated VS Code window */}
      <div
        style={{
          position: "absolute",
          left: codeWin.localX,
          top: codeWin.localY,
          width: codeWin.localWidth,
          height: codeWin.localHeight,
          borderRadius: 10,
          background: "#14161c",
          boxShadow: "0 24px 60px rgba(0,0,0,0.55), inset 0 0 0 1px rgba(255,255,255,0.08)",
          overflow: "hidden",
          display: "flex",
          flexDirection: "column",
        }}
      >
        <div
          style={{
            height: 36,
            padding: "0 14px",
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            background: "#1a1d26",
            borderBottom: "1px solid rgba(255,255,255,0.06)",
            fontSize: 12,
            color: "#b4bac8",
          }}
        >
          <span>{codeWin.title}</span>
          <span style={{ fontFamily: "var(--font-mono)", fontSize: 11, color: "#7f8799" }}>Rust · UTF-8</span>
        </div>
        <pre
          style={{
            margin: 0,
            padding: "18px 20px",
            fontFamily: "var(--font-mono)",
            fontSize: 13,
            lineHeight: 1.65,
            color: "#d4d8e3",
            overflow: "hidden",
          }}
        >
          {`pub fn detect_circle(points: &[(f64, f64)]) -> Option<Rect> {
    if points.len() < 5 {
        return None;
    }
    // Check closed loop: bbox >= 40x40, turning >= 300°,
    // end near start relative to diagonal, monotonic orbit.
    let net_turn = signed_turn.abs();
    if net_turn >= MIN_TURNING_RAD && monotonicity >= 0.72 {
        return Some(bbox);
    }
    None
}`}
        </pre>
      </div>

      {/* Simulated Terminal window (higher Z-order) */}
      <div
        style={{
          position: "absolute",
          left: termWin.localX,
          top: termWin.localY,
          width: termWin.localWidth,
          height: termWin.localHeight,
          borderRadius: 10,
          background: "#0c0e13",
          boxShadow: "0 24px 64px rgba(0,0,0,0.65), inset 0 0 0 1px rgba(255,255,255,0.1)",
          overflow: "hidden",
          display: "flex",
          flexDirection: "column",
        }}
      >
        <div
          style={{
            height: 34,
            padding: "0 12px",
            display: "flex",
            alignItems: "center",
            background: "#151820",
            borderBottom: "1px solid rgba(255,255,255,0.06)",
            fontSize: 12,
            color: "#a8b0c0",
          }}
        >
          {termWin.title}
        </div>
        <pre
          style={{
            margin: 0,
            padding: "14px 16px",
            fontFamily: "var(--font-mono)",
            fontSize: 12,
            lineHeight: 1.6,
            color: "#9ae6b4",
          }}
        >
          {`$ cargo test --lib area
running 6 tests
test area::tests::normalizes_and_clamps ... ok
test area::tests::converts_logical_and_physical ... ok
test area::tests::detects_circles ... ok
test area::tests::detects_ellipses ... ok
test area::tests::rejects_lines_zigzags ... ok

test result: ok. 6 passed; 0 failed`}
        </pre>
      </div>

      <AreaPickApp
        qaState={qaState}
        onQaConfirmRect={(rect, phys) =>
          setToast(
            `Выбрана область ${phys.width}×${phys.height} физ. px (${Math.round(rect.x)}, ${Math.round(rect.y)})`,
          )
        }
        onQaConfirmWindow={(win) =>
          setToast(`Выбрано окно «${win.title}» (${win.rect.width}×${win.rect.height})`)
        }
        onQaCancel={() => setToast("Отменено (Esc / правый клик)")}
      />

      {toast && (
        <div
          style={{
            position: "fixed",
            bottom: 22,
            left: "50%",
            transform: "translateX(-50%)",
            zIndex: 40,
            padding: "8px 16px",
            borderRadius: 999,
            background: "rgba(8, 8, 10, 0.94)",
            boxShadow: "0 8px 28px rgba(0,0,0,0.5), inset 0 0 0 1px rgba(255,178,63,0.45)",
            color: "#ffd88f",
            fontSize: 12.5,
            fontWeight: 540,
            pointerEvents: "none",
          }}
        >
          {toast}
        </div>
      )}
    </div>
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <QaDesktop />
  </StrictMode>,
);
