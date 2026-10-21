// The Dynamic Island: one continuous surface that morphs between states.
// Geometry is driven by springs; the native hit-tester receives the target
// rect on every state change so the transparent canvas never blocks clicks.

import { AnimatePresence, motion, useMotionValue, useReducedMotion, useSpring, useTransform } from "motion/react";
import { useEffect, useMemo, useRef, type PointerEvent as ReactPointerEvent } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { ipc, on } from "../lib/ipc";
import { resolveMode, useIsland } from "../store/island";
import { hasActiveMedia, useSystem } from "../store/system";
import { useAgentMood } from "../store/agent";
import { BROW_H, CANVAS_W, SHOULDER, bottomRadius, dimsFor, notchPath, shapeRect, type IslandMode } from "./geometry";
import { Eyes, type EyeMood } from "./Eyes";
import { CompactView } from "./views/CompactView";
import { PeekView } from "./views/PeekView";
import { DropView } from "./views/DropView";
import { WorkspaceView } from "../workspace/WorkspaceView";
import "./island.css";

const SHAPE_SPRING = { stiffness: 380, damping: 28, mass: 0.8 };
const REDUCED = { stiffness: 1000, damping: 100, mass: 1 };

function useIslandController() {
  const enterTimer = useRef<number | undefined>(undefined);
  const leaveTimer = useRef<number | undefined>(undefined);

  useEffect(() => {
    const set = useIsland.getState().set;
    const unsubs: Array<Promise<() => void>> = [];

    unsubs.push(
      on("island:pointer", ({ inside, dragApproach }) => {
        set({ dragApproach });
        window.clearTimeout(enterTimer.current);
        window.clearTimeout(leaveTimer.current);
        if (inside) {
          // A short dwell avoids expanding when the cursor merely passes by
          // on its way to a title bar.
          enterTimer.current = window.setTimeout(() => set({ hovered: true }), 70);
        } else {
          leaveTimer.current = window.setTimeout(() => set({ hovered: false }), 320);
        }
      }),
    );
    unsubs.push(on("island:presence", ({ visible }) => set({ visible })));
    unsubs.push(on("island:blur", () => set({ workspaceOpen: false, hovered: false })));
    unsubs.push(
      on("island:command", (cmd) => {
        const s = useIsland.getState();
        if (cmd === "toggle-workspace") set({ workspaceOpen: !s.workspaceOpen });
        else if (cmd === "open-workspace") set({ workspaceOpen: true });
      }),
    );
    unsubs.push(
      getCurrentWebview().onDragDropEvent((e) => {
        const p = e.payload;
        if (p.type === "enter" || p.type === "over") set({ dragOver: true });
        else if (p.type === "leave") set({ dragOver: false });
        else if (p.type === "drop") {
          const files = p.paths.map((path) => ({ path, name: path.split(/[\\/]/).pop() ?? path }));
          useIsland.getState().addAttachments(files);
          set({ dragOver: false, workspaceOpen: true });
        }
      }),
    );

    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        const s = useIsland.getState();
        if (s.workspaceOpen) set({ workspaceOpen: false });
        else if (s.voiceOpen) set({ voiceOpen: false });
        else if (s.hovered) set({ hovered: false });
      }
    };
    window.addEventListener("keydown", onKey);

    return () => {
      window.removeEventListener("keydown", onKey);
      unsubs.forEach((u) => u.then((f) => f()));
    };
  }, []);

  // Keyboard focus follows the workspace: grab it on open, give it back on close.
  const workspaceOpen = useIsland((s) => s.workspaceOpen);
  const first = useRef(true);
  useEffect(() => {
    if (first.current) {
      first.current = false;
      return;
    }
    if (workspaceOpen) void ipc.island.beginInput();
    else void ipc.island.endInput(true);
  }, [workspaceOpen]);
}

function moodFor(mode: IslandMode, agentMood: EyeMood | null, hovered: boolean): EyeMood {
  if (agentMood) return agentMood;
  if (mode === "drop") return "attentive";
  if (mode === "peek" || mode === "workspace" || hovered) return "attentive";
  return "idle";
}

export function Island() {
  useIslandController();
  const st = useIsland();
  const media = useSystem((s) => s.media);
  const refreshControls = useSystem((s) => s.refreshControls);
  const agentMood = useAgentMood();
  const reduced = useReducedMotion();

  const mode = resolveMode(st);
  const withMedia = hasActiveMedia(media);
  const dims = useMemo(() => dimsFor(mode, { hasMedia: withMedia, hasActivity: false }), [mode, withMedia]);

  const spring = reduced ? REDUCED : SHAPE_SPRING;
  const w = useSpring(dims.w, spring);
  const h = useSpring(dims.h, spring);
  useEffect(() => {
    w.set(dims.w);
    h.set(dims.h);
    void ipc.island.setShape(shapeRect(dims));
  }, [dims, w, h]);

  // Fresh, verified values every time the controls become visible.
  useEffect(() => {
    if (mode === "peek") void refreshControls();
  }, [mode, refreshControls]);

  const r = useTransform(h, (hv) => bottomRadius(hv));
  const d = useTransform(() => notchPath(w.get(), h.get(), r.get(), SHOULDER));
  const svgW = useTransform(w, (v) => v + SHOULDER * 2);

  // Gaze follows the pointer while it is over the island.
  const lookX = useSpring(0, { stiffness: 200, damping: 24 });
  const onPointerMove = (e: ReactPointerEvent) => {
    const cx = CANVAS_W / 2;
    lookX.set(Math.max(-1, Math.min(1, (e.clientX - cx) / (dims.w / 2))));
  };
  const zero = useMotionValue(0);

  return (
    <div className={`stage${st.visible ? "" : " is-hidden"}`}>
      <motion.div className="island" style={{ width: w, height: h }} onPointerMove={onPointerMove} onPointerLeave={() => lookX.set(0)}>
        <motion.svg className="island-shape" style={{ width: svgW, height: h, left: -SHOULDER }} aria-hidden="true">
          <defs>
            <linearGradient id="rim" x1="0" y1="0" x2="0" y2="1">
              <stop offset="0" stopColor="#ffffff" stopOpacity="0" />
              <stop offset="0.6" stopColor="#ffffff" stopOpacity="0.04" />
              <stop offset="1" stopColor="#ffffff" stopOpacity="0.16" />
            </linearGradient>
            <radialGradient id="sheen" cx="0.5" cy="0" r="0.9">
              <stop offset="0" stopColor="#1d1d22" />
              <stop offset="0.7" stopColor="#0b0b0d" />
              <stop offset="1" stopColor="#08080a" />
            </radialGradient>
          </defs>
          <motion.path d={d} fill="url(#sheen)" stroke="url(#rim)" strokeWidth={1} />
        </motion.svg>

        <motion.div className="island-clip" style={{ borderBottomLeftRadius: r, borderBottomRightRadius: r }}>
          <AnimatePresence initial={false} mode="popLayout">
            <motion.div
              key={mode}
              className="island-content"
              style={{ width: dims.w, height: dims.h }}
              initial={{ opacity: 0, filter: "blur(6px)", scale: 0.97 }}
              animate={{ opacity: 1, filter: "blur(0px)", scale: 1, transition: { duration: 0.24, delay: 0.06, ease: [0.22, 1, 0.36, 1] } }}
              exit={{ opacity: 0, filter: "blur(4px)", scale: 0.98, transition: { duration: 0.1 } }}
            >
              {mode === "compact" && <CompactView />}
              {mode === "peek" && <PeekView />}
              {mode === "drop" && <DropView />}
              {mode === "workspace" && <WorkspaceView />}
            </motion.div>
          </AnimatePresence>
        </motion.div>

        <div className="island-eyes" style={{ height: BROW_H }}>
          <Eyes mood={moodFor(mode, agentMood, st.hovered)} lookX={mode === "compact" ? zero : lookX} lookY={mode === "drop" ? 2 : 0} />
        </div>
      </motion.div>
    </div>
  );
}
