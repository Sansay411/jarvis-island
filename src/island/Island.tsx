// The Dynamic Island: one continuous surface that morphs between states.
// Geometry is driven by springs; the native hit-tester receives the target
// rect on every state change so the transparent canvas never blocks clicks.

import { AnimatePresence, motion, useMotionValue, useReducedMotion, useSpring, useTransform } from "motion/react";
import { useEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { invoke } from "@tauri-apps/api/core";
import { ipc, on } from "../lib/ipc";
import { resolveMode, useIsland } from "../store/island";
import { hasActiveMedia, useSystem } from "../store/system";
import { useRunningJobs } from "../store/jobs";
import { useAgent, useAgentMood } from "../store/agent";
import { usePrefs } from "../store/prefs";
import { BROW_H, CANVAS_W, bottomRadius, dimsFor, notchPath, shapeRect, shoulderFor, type IslandMode } from "./geometry";
import { Eyes, type EyeMood } from "./Eyes";
import { CompactView } from "./views/CompactView";
import { PeekView } from "./views/PeekView";
import { DropView } from "./views/DropView";
import { VoiceView } from "./views/VoiceView";
import { FilesView } from "./views/FilesView";
import { closeFiles, showFiles, useFiles } from "../store/files";
import { NoticeView } from "./views/NoticeView";
import { useAlerts, useNextTimer } from "../store/reminders";
import { useStopwatch } from "../store/stopwatch";
import { emote, useEmote } from "../store/emote";
import { useClockRows } from "./views/ClocksStrip";
import { inputLevel, outputLevel, toggleVoice, useVoice, type VoiceState } from "../store/voice";
import { WorkspaceView } from "../workspace/WorkspaceView";
import "./island.css";

const SHAPE_SPRING = { stiffness: 460, damping: 34, mass: 0.65 };

/** Measure real frame pacing for ~700 ms after a shape change and log it
 *  natively (Settings → Diagnostics reads the log). Costs nothing at rest. */
function sampleFrames(label: string): () => void {
  let raf = 0;
  const times: number[] = [];
  const start = performance.now();
  const step = (t: number) => {
    times.push(t);
    if (t - start < 700) raf = requestAnimationFrame(step);
    else if (times.length > 2) {
      const gaps = times.slice(1).map((v, i) => v - times[i]!);
      const avg = gaps.reduce((a, b) => a + b, 0) / gaps.length;
      void invoke("perf_report", { label, fps: Math.round(1000 / avg), worstMs: Math.round(Math.max(...gaps)), frames: gaps.length });
    }
  };
  raf = requestAnimationFrame(step);
  return () => cancelAnimationFrame(raf);
}
const REDUCED = { stiffness: 1000, damping: 100, mass: 1 };

function useIslandController() {
  const enterTimer = useRef<number | undefined>(undefined);
  const leaveTimer = useRef<number | undefined>(undefined);

  useEffect(() => {
    const set = useIsland.getState().set;
    const unsubs: Array<Promise<() => void>> = [];

    unsubs.push(
      on("island:pointer", ({ inside }) => {
        window.clearTimeout(enterTimer.current);
        window.clearTimeout(leaveTimer.current);
        if (inside) {
          // A short dwell avoids expanding when the cursor merely passes by
          // on its way to a title bar.
          // Over a maximised window the lip waits longer, so reaching for the
          // app's own title-bar controls doesn't pop the island open.
          const dwell = useIsland.getState().topCovered ? 150 : 45;
          enterTimer.current = window.setTimeout(() => set({ hovered: true }), dwell);
        } else {
          leaveTimer.current = window.setTimeout(() => set({ hovered: false }), 320);
        }
      }),
    );
    unsubs.push(on("island:presence", ({ visible }) => set({ visible })));
    unsubs.push(on("island:context", ({ topCovered }) => set({ topCovered })));
    void invoke<{ topCovered: boolean }>("island_context").then(({ topCovered }) => set({ topCovered }));
    unsubs.push(on("island:blur", () => set({ workspaceOpen: false, hovered: false })));
    unsubs.push(
      on("island:command", (cmd) => {
        const s = useIsland.getState();
        if (cmd === "toggle-workspace") set({ workspaceOpen: !s.workspaceOpen });
        else if (cmd === "open-workspace") set({ workspaceOpen: true });
        else if (cmd === "toggle-voice") void toggleVoice();
      }),
    );
    // Drops arrive from our own drop target (re-armed as a drag approaches)
    // and, until then, from Tauri's.
    const onFileDrag = (type: string, paths: string[]) => {
      if (type === "enter" || type === "over") {
        // A file arrives: the eyes widen once.
        if (!useIsland.getState().dragOver) emote("surprised");
        set({ dragOver: true });
      }
      else if (type === "leave") set({ dragOver: false });
      else if (type === "drop") {
        set({ dragOver: false });
        // Into an open chat: attach. Elsewhere: offer actions for the file.
        if (useIsland.getState().workspaceOpen) {
          useIsland.getState().addAttachments(paths.map((path) => ({ path, name: path.split(/[\\/]/).pop() ?? path })));
        } else if (paths.length > 0) void showFiles(paths);
      }
    };
    unsubs.push(on("island:file-drop", ({ type, paths }) => onFileDrag(type, paths)));
    unsubs.push(getCurrentWebview().onDragDropEvent(({ payload: p }) => onFileDrag(p.type, "paths" in p ? p.paths : [])));

    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        const s = useIsland.getState();
        if (useFiles.getState().open) closeFiles();
        else if (s.workspaceOpen) set({ workspaceOpen: false });
        else if (s.voiceOpen) void invoke("voice_stop");
        else if (s.hovered) set({ hovered: false });
      }
    };
    window.addEventListener("keydown", onKey);

    let disposed = false;
    void Promise.all(unsubs).then(() => {
      if (!disposed) return invoke("island_frontend_ready");
    }).catch(console.error);

    return () => {
      disposed = true;
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
    if (workspaceOpen) void ipc.island.beginInput().then(() => window.dispatchEvent(new Event("jarvis:input-ready")));
    else void ipc.island.endInput(true);
  }, [workspaceOpen]);
}

/** After 23:00 and before 6:00 the eyes rest heavy-lidded. */
function useNight(): boolean {
  const [night, setNight] = useState(() => isNight());
  useEffect(() => {
    const id = window.setInterval(() => setNight(isNight()), 60_000);
    return () => window.clearInterval(id);
  }, []);
  return night;
}

function isNight(): boolean {
  const h = new Date().getHours();
  return h >= 23 || h < 6;
}

/** True during the last ten seconds of the running timer. */
function useTimerFinale(dueAt: string | null): boolean {
  const [on, setOn] = useState(false);
  useEffect(() => {
    setOn(false);
    if (!dueAt) return;
    const left = Date.parse(dueAt) - Date.now();
    if (left <= 0) return;
    const start = window.setTimeout(() => setOn(true), Math.max(0, left - 10_000));
    const end = window.setTimeout(() => setOn(false), left);
    return () => {
      window.clearTimeout(start);
      window.clearTimeout(end);
    };
  }, [dueAt]);
  return on;
}

/** In a voice conversation the eyes listen, think and speak with it. */
function voiceMood(state: VoiceState, agentMood: EyeMood | null): EyeMood {
  if (state === "error") return "error";
  if (agentMood === "happy" || agentMood === "error") return agentMood;
  if (state === "speaking") return "speaking";
  if (state === "thinking") return "thinking";
  if (state === "hearing" || state === "listening") return "listening";
  return "attentive";
}

function moodFor(mode: IslandMode, agentMood: EyeMood | null, hovered: boolean): EyeMood {
  if (agentMood) return agentMood;
  if (mode === "drop" || mode === "files" || mode === "notice") return "attentive";
  if (mode === "peek" || mode === "workspace" || hovered) return "attentive";
  return "idle";
}

export function Island() {
  useIslandController();
  const st = useIsland();
  const media = useSystem((s) => s.media);
  const refreshControls = useSystem((s) => s.refreshControls);
  const agentMood = useAgentMood();
  const reduced = useReducedMotion() || usePrefs((p) => p.reduceMotion);

  const filesOpen = useFiles((s) => s.open);
  const filesH = useFiles((s) => s.height);
  const alerting = useAlerts().length > 0;
  const timer = useNextTimer();
  const stopwatch = useStopwatch((s) => s.active);
  const { emote: currentEmote, seq: emoteSeq } = useEmote();
  const night = useNight();
  const voiceState = useVoice((s) => s.state);
  // The eyes take the mic level while listening and JARVIS's own while speaking.
  const speaking = useRef(false);
  speaking.current = voiceState === "speaking";
  const voiceLevel = useTransform(() => (speaking.current ? outputLevel.get() : inputLevel.get()));
  const timerFinale = useTimerFinale(timer?.dueAt ?? null);
  const clockRows = useClockRows();
  const base = resolveMode(st);
  // Drop and voice keep priority; a fired reminder shows over the rest.
  const mode: IslandMode = base === "drop" ? "drop" : filesOpen ? "files" : base === "voice" ? "voice" : alerting ? "notice" : base;
  const [workspaceWarm,setWorkspaceWarm]=useState(false);
  useEffect(() => {
    const id=window.requestIdleCallback(()=>setWorkspaceWarm(true),{timeout:1200});
    return ()=>window.cancelIdleCallback(id);
  },[]);
  useEffect(()=>{ if(mode==="workspace") setWorkspaceWarm(true); },[mode]);
  const withMedia = hasActiveMedia(media);
  const jobRows = Math.min(2, useRunningJobs().length);
  const confirmPending = useAgent((s) => s.items.some((it) => it.kind === "tool" && it.status === "awaiting"));
  const dims = useMemo(
    () => dimsFor(mode, { hasMedia: withMedia, hasActivity: jobRows > 0 || !!timer || stopwatch, jobRows, clockRows, lip: st.topCovered, confirm: confirmPending, filesH }),
    [mode, withMedia, jobRows, clockRows, st.topCovered, confirmPending, filesH, !!timer, stopwatch],
  );

  const lip = mode === "compact" && st.topCovered;
  const workspaceDims=dimsFor("workspace",{hasMedia:withMedia,hasActivity:jobRows>0,jobRows,lip:false,confirm:confirmPending});
  const spring = reduced ? REDUCED : SHAPE_SPRING;
  const w = useSpring(dims.w, spring);
  const h = useSpring(dims.h, spring);
  useEffect(() => {
    w.set(dims.w);
    h.set(dims.h);
    void ipc.island.setShape(shapeRect(dims));
    return sampleFrames(`${mode} ${dims.w}x${dims.h}`);
  }, [dims, w, h]);

  // Fresh, verified values every time the controls become visible.
  useEffect(() => {
    if (mode === "peek") void refreshControls();
  }, [mode, refreshControls]);

  const r = useTransform(h, (hv) => bottomRadius(hv));
  const d = useTransform(() => notchPath(w.get(), h.get(), r.get(), shoulderFor(h.get())));
  const svgW = useTransform(() => w.get() + shoulderFor(h.get()) * 2);
  const svgLeft = useTransform(h, (hv) => -shoulderFor(hv));

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
        <motion.svg className="island-shape" style={{ width: svgW, height: h, left: svgLeft }} aria-hidden="true">
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
            {mode !== "workspace" && <motion.div
              key={mode}
              className="island-content"
              style={{ width: dims.w, height: dims.h }}
              // Opacity/scale keep the small views on the compositor;
              // the workspace stays mounted separately below.
              initial={{ opacity: 0, scale: 0.99 }}
              animate={{ opacity: 1, scale: 1, transition: { duration: reduced ? 0 : 0.14, ease: [0.22, 1, 0.36, 1] } }}
              exit={{ opacity: 0, transition: { duration: reduced ? 0 : 0.07 } }}
            >
              {mode === "compact" && <CompactView />}
              {mode === "peek" && <PeekView />}
              {mode === "drop" && <DropView />}
              {mode === "voice" && <VoiceView />}
              {mode === "files" && <FilesView />}
              {mode === "notice" && <NoticeView />}
            </motion.div>}
          </AnimatePresence>
          {(workspaceWarm || mode==="workspace") && <motion.div
            className="island-content"
            style={{width:workspaceDims.w,height:workspaceDims.h,visibility:mode==="workspace"?"visible":"hidden",pointerEvents:mode==="workspace"?"auto":"none"}}
            aria-hidden={mode!=="workspace"}
            inert={mode!=="workspace"}
            initial={false}
            animate={{opacity:mode==="workspace"?1:0}}
            transition={{duration:reduced?0:0.12}}
          ><WorkspaceView visible={mode==="workspace"}/></motion.div>}
        </motion.div>

        <div className="island-eyes" style={{ height: lip ? dims.h : BROW_H }}>
          <Eyes
            mood={mode === "voice" ? voiceMood(voiceState, agentMood) : moodFor(mode, agentMood, st.hovered)}
            level={voiceLevel}
            lookX={mode === "compact" ? zero : lookX}
            lookY={mode === "drop" ? 2 : 0}
            scale={lip ? 0.36 : 1}
            emote={currentEmote}
            emoteSeq={emoteSeq}
            bob={media?.status === "playing"}
            sleepy={night}
            watch={timerFinale && (mode === "compact" || mode === "peek")}
            alarm={mode === "notice"}
            onBoop={lip ? undefined : () => emote("boop")}
          />
        </div>
      </motion.div>
    </div>
  );
}
