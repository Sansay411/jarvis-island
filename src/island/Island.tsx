// The Dynamic Island: one continuous hardware surface that morphs between states.
// Geometry is driven by springs; the native hit-tester receives the target
// rect on every state change so the transparent canvas never blocks clicks.

import { AnimatePresence, motion, useMotionValue, useReducedMotion, useSpring, useTransform } from "motion/react";
import { useEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ipc, isTauri, on } from "../lib/ipc";
import { endInlineInput, resolveMode, useIsland } from "../store/island";
import { hasActiveMedia, useSystem } from "../store/system";
import { useRunningJobs } from "../store/jobs";
import { useAgent, useAgentMood } from "../store/agent";
import { usePrefs } from "../store/prefs";
import { useHub } from "../store/hub";
import { useTts } from "../store/tts";
import { BROW_H, CANVAS_W, bottomRadius, dimsFor, notchLayout, notchPath, shapeRect, shoulderFor, type IslandMode, type NotchGeometry } from "./geometry";
import { Eyes, type EyeMood } from "./Eyes";
import { CompactView } from "./views/CompactView";
import { PeekView } from "./views/PeekView";
import { DropView } from "./views/DropView";
import { VoiceView, useVoiceClock } from "./views/VoiceView";
import { FilesView } from "./views/FilesView";
import { ClipsView } from "./views/ClipsView";
import { useMissionActive } from "./views/AgentMissionBar";
import { closeClips, toggleClips, useClips } from "../store/clips";
import { closeFiles, runFileAction, showFiles, useFiles } from "../store/files";
import { dropGoesToSend, isOwnDrag } from "./dragGuard";
import { WeatherView } from "./views/WeatherView";
import { useWeather } from "../store/weather";
import { NoticeView } from "./views/NoticeView";
import { useAlerts, useNextTimer } from "../store/reminders";
import { useStopwatch } from "../store/stopwatch";
import { activityOf, emote, useEmote } from "../store/emote";
import { useClockRows } from "./views/ClocksStrip";
import { inputLevel, outputLevel, toggleVoice, useVoice, type VoiceState } from "../store/voice";
import { WorkspaceView } from "../workspace/WorkspaceView";
import { HeadsetHello } from "./views/SoundDeck";
import { useBluetooth } from "../store/bluetooth";
import { UpdateCard } from "./views/UpdateCard";
import { useUpdates } from "../store/updates";
import "./island.css";

const SHAPE_SPRING = { stiffness: 460, damping: 34, mass: 0.65 };
const REDUCED = { stiffness: 1000, damping: 100, mass: 1 };

/** Measure real frame pacing for ~700 ms after a shape change and log it
 *  natively (Settings → Diagnostics reads the log). Costs nothing at rest. */
function sampleFrames(label: string): () => void {
  if (!isTauri()) return () => {};
  let raf = 0;
  const times: number[] = [];
  const start = performance.now();
  const step = (t: number) => {
    times.push(t);
    if (t - start < 700) raf = requestAnimationFrame(step);
    else if (times.length > 2) {
      const gaps = times.slice(1).map((v, i) => v - times[i]!);
      const avg = gaps.reduce((a, b) => a + b, 0) / gaps.length;
      void invoke("perf_report", {
        label,
        fps: Math.round(1000 / avg),
        worstMs: Math.round(Math.max(...gaps)),
        frames: gaps.length,
      }).catch(() => {});
    }
  };
  raf = requestAnimationFrame(step);
  return () => cancelAnimationFrame(raf);
}

function useIslandController(nativeGeometry: NotchGeometry | null) {
  const macIsland = useRef(nativeGeometry !== null);
  useEffect(() => { macIsland.current = nativeGeometry !== null; }, [nativeGeometry]);
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
          const dwell = useIsland.getState().topCovered ? 150 : 45;
          enterTimer.current = window.setTimeout(() => set({ hovered: true }), dwell);
        } else {
          leaveTimer.current = window.setTimeout(() => {
            const s = useIsland.getState();
            // Keep open while typing inside an inline input on the notch.
            if (s.inputFocused) return;
            set({ hovered: false, focusBannerDismissed: false });
            if (s.clipsOpen && !s.clipsKeyboard) closeClips();
          }, 320);
        }
      }),
    );
    unsubs.push(on("island:presence", ({ visible }) => set({ visible })));
    unsubs.push(on("island:context", ({ topCovered }) => set({ topCovered })));
    if (isTauri()) {
      void invoke<{ topCovered: boolean }>("island_context")
        .then(({ topCovered }) => set({ topCovered }))
        .catch(() => {});
    }
    unsubs.push(
      on("island:blur", () =>
        set({
          workspaceOpen: false,
          hovered: false,
          clipsOpen: false,
          clipsKeyboard: false,
          inputFocused: false,
          focusBannerDismissed: false,
        }),
      ),
    );
    unsubs.push(
      on("island:command", (cmd) => {
        const s = useIsland.getState();
        if (cmd === "toggle-workspace") set({ workspaceOpen: !s.workspaceOpen });
        else if (cmd === "open-workspace") set({ workspaceOpen: true });
        else if (cmd === "toggle-voice") void toggleVoice();
        else if (cmd === "toggle-clips") toggleClips();
      }),
    );

    // Physical pixels inside the island → is the cursor over «В Telegram»?
    const overSend = (x?: number | null, y?: number | null): boolean | null => {
      if (x == null || y == null) return null;
      const el = document.querySelector<HTMLElement>('[data-drop-target="telegram"]');
      if (!el) return null;
      const r = el.getBoundingClientRect();
      const cx = x / window.devicePixelRatio;
      const cy = y / window.devicePixelRatio;
      return cx >= r.left && cx <= r.right && cy >= r.top && cy <= r.bottom;
    };

    const onFileDrag = (type: string, paths: string[], x?: number | null, y?: number | null) => {
      // Our own drag-out passing over the island is not a drop onto it.
      if (isOwnDrag(paths)) {
        if (useIsland.getState().dragOver) set({ dragOver: false, dropOnSend: false });
        return;
      }
      if (type === "enter" || type === "over") {
        if (!useIsland.getState().dragOver) emote("surprised");
        set({ dragOver: true, dropOnSend: overSend(x, y) ?? useIsland.getState().dropOnSend });
      } else if (type === "leave") {
        set({ dragOver: false, dropOnSend: false });
      } else if (type === "drop") {
        const toSend = dropGoesToSend(overSend(x, y), useIsland.getState().dropOnSend);
        set({ dragOver: false, dropOnSend: false });
        const cur = useIsland.getState();
        if (toSend && paths.length > 0) {
          // The file card follows the send and shows the verified result.
          void showFiles(paths).then(() => runFileAction("telegram", paths));
          emote("wink");
        } else if (cur.workspaceOpen) {
          cur.addAttachments(paths.map((path) => ({ path, name: path.split(/[\\/]/).pop() ?? path })));
        } else if (cur.hovered && cur.peekDeck === "tray" && paths.length > 0) {
          void useHub.getState().shelfAddFiles(paths);
          emote("wink");
        } else if (paths.length > 0) {
          void showFiles(paths);
        }
      }
    };

    unsubs.push(on("island:file-drop", ({ type, paths, x, y }) => onFileDrag(type, paths, x, y)));
    if (isTauri()) {
      unsubs.push(
        getCurrentWebview().onDragDropEvent(({ payload: p }) =>
          onFileDrag(p.type, "paths" in p ? p.paths : [], "position" in p ? p.position.x : null, "position" in p ? p.position.y : null),
        ),
      );
    }

    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        const s = useIsland.getState();
        if (macIsland.current && s.inputFocused && !s.workspaceOpen && !s.clipsOpen) {
          e.preventDefault();
          window.clearTimeout(enterTimer.current);
          void endInlineInput(() => {
            if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
          }).catch(console.error);
          return;
        }
        if (useFiles.getState().open) closeFiles();
        else if (s.workspaceOpen) set({ workspaceOpen: false });
        else if (s.voiceOpen) {
          if (isTauri()) void invoke("voice_stop");
          else set({ voiceOpen: false });
        } else if (useTts.getState().speaking) {
          void useTts.getState().stop();
        } else if (s.clipsOpen) closeClips();
        else if (s.hovered) set({ hovered: false, focusBannerDismissed: false });
      }
    };
    window.addEventListener("keydown", onKey);

    let disposed = false;
    if (isTauri()) {
      void Promise.all(unsubs)
        .then(() => {
          if (!disposed) return invoke("island_frontend_ready");
        })
        .catch(console.error);
    }

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

  const clipsKeyboard = useIsland((s) => s.clipsOpen && s.clipsKeyboard);
  const clipsInput = useRef(false);
  useEffect(() => {
    if (clipsKeyboard === clipsInput.current) return;
    clipsInput.current = clipsKeyboard;
    if (clipsKeyboard) void ipc.island.beginInput().then(() => window.dispatchEvent(new Event("jarvis:input-ready")));
    else if (!useIsland.getState().workspaceOpen) void ipc.island.endInput(true);
  }, [clipsKeyboard]);

  useEffect(() => {
    if (workspaceOpen) closeClips();
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
  const [onState, setOnState] = useState(false);
  useEffect(() => {
    setOnState(false);
    if (!dueAt) return;
    const left = Date.parse(dueAt) - Date.now();
    if (left <= 0) return;
    const start = window.setTimeout(() => setOnState(true), Math.max(0, left - 10_000));
    const end = window.setTimeout(() => setOnState(false), left);
    return () => {
      window.clearTimeout(start);
      window.clearTimeout(end);
    };
  }, [dueAt]);
  return onState;
}

function voiceMood(state: VoiceState, agentMood: EyeMood | null): EyeMood {
  if (state === "error") return "error";
  if (agentMood === "happy" || agentMood === "error") return agentMood;
  if (state === "speaking") return "speaking";
  if (state === "thinking") return "thinking";
  if (state === "hearing" || state === "listening") return "listening";
  return "attentive";
}

function moodFor(mode: IslandMode, agentMood: EyeMood | null, hovered: boolean, ttsSpeaking: boolean): EyeMood {
  if (ttsSpeaking) return "speaking";
  if (agentMood) return agentMood;
  if (mode === "drop" || mode === "files" || mode === "notice" || mode === "clips" || mode === "weather" || mode === "hello") return "attentive";
  if (mode === "peek" || mode === "workspace" || hovered) return "attentive";
  return "idle";
}

export function Island() {
  const [notchGeometry, setNotchGeometry] = useState<NotchGeometry | null>(null);
  useEffect(() => {
    if (!isTauri()) return;
    let active = true;
    const update = (value: NotchGeometry | null) => { if (active) setNotchGeometry(value); };
    const unlisten = listen<NotchGeometry>("island:notch-geometry", (event) => update(event.payload));
    void invoke<NotchGeometry | null>("island_notch_geometry").then(update).catch(() => {});
    return () => { active = false; void unlisten.then((off) => off()); };
  }, []);
  const notch = notchLayout(notchGeometry);
  useIslandController(notchGeometry);
  const st = useIsland();
  const media = useSystem((s) => s.media);
  const artworkRgb = useSystem((s) => s.artworkRgb);
  const refreshControls = useSystem((s) => s.refreshControls);
  const agentMood = useAgentMood();
  const reduced = useReducedMotion() || usePrefs((p) => p.reduceMotion);

  const filesOpen = useFiles((s) => s.open);
  const filesH = useFiles((s) => s.height);
  const clipsH = useClips((s) => s.height);
  const alerting = useAlerts().length > 0;
  const timer = useNextTimer();
  const stopwatch = useStopwatch((s) => s.active);
  const focus = useHub((s) => s.focus);
  const ttsSpeaking = useTts((s) => s.speaking);
  const missionActive = useMissionActive();

  const { emote: currentEmote, seq: emoteSeq } = useEmote();
  const runningTool = useAgent((s) => {
    const it = s.items.findLast((i) => i.kind === "tool" && i.status === "running");
    return it?.kind === "tool" ? it.tool : null;
  });
  const activity = runningTool ? activityOf(runningTool) : null;
  const night = useNight();
  const voiceState = useVoice((s) => s.state);

  // Eyes follow live `outputLevel` whenever voice session OR standalone TTS is speaking.
  const speaking = useRef(false);
  speaking.current = voiceState === "speaking" || ttsSpeaking;
  const fallbackPulse = useMotionValue(0);
  useEffect(() => {
    if (!ttsSpeaking || reduced) {
      fallbackPulse.set(0);
      return;
    }
    let raf = 0;
    const t0 = performance.now();
    const tick = (now: number) => {
      // Yield 100% to real PCM output amplitude if `voice:level` is active.
      if (outputLevel.get() > 0.025) {
        fallbackPulse.set(0);
      } else {
        const dt = (now - t0) / 1000;
        fallbackPulse.set(0.32 + 0.24 * Math.sin(dt * 10.5) + 0.14 * Math.sin(dt * 4.3));
      }
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [ttsSpeaking, reduced, fallbackPulse]);

  const voiceLevel = useTransform(() =>
    speaking.current ? Math.max(outputLevel.get(), fallbackPulse.get()) : inputLevel.get(),
  );

  const timerFinale = useTimerFinale(timer?.dueAt ?? null);
  const clockRows = useClockRows();
  const voiceClock = useVoiceClock();
  const base = resolveMode(st);
  const weatherCard = useWeather((s) => s.card !== null);
  const helloCard = useBluetooth((s) => s.hello !== null);
  const updateCard = useUpdates((s) => s.card && s.status?.available != null);
  const mode: IslandMode =
    base === "drop"
      ? "drop"
      : filesOpen
        ? "files"
        : weatherCard && base !== "workspace"
          ? "weather"
          : base === "voice"
            ? "voice"
            : alerting
              ? "notice"
              : (helloCard || updateCard) && base === "compact"
                ? "hello"
                : base;

  const [workspaceWarm, setWorkspaceWarm] = useState(false);
  useEffect(() => {
    if (!isTauri()) return;
    if (typeof window.requestIdleCallback === "function") {
      const id = window.requestIdleCallback(() => setWorkspaceWarm(true), { timeout: 1200 });
      return () => window.cancelIdleCallback(id);
    }
    const id = window.setTimeout(() => setWorkspaceWarm(true), 800);
    return () => window.clearTimeout(id);
  }, []);
  useEffect(() => {
    if (mode === "workspace") setWorkspaceWarm(true);
  }, [mode]);

  const withMedia = hasActiveMedia(media);
  const agentThinking = useAgent((s) => Boolean(s.activeTurn));
  const jobRows = Math.min(2, useRunningJobs().length);
  const confirmPending = useAgent((s) => s.items.some((it) => it.kind === "tool" && it.status === "awaiting"));
  const showFocusBanner = st.forceFocusBanner || (focus.active && !st.focusBannerDismissed);

  const dims = useMemo(
    () =>
      dimsFor(mode, {
        notch: notchGeometry,
        hasMedia: withMedia,
        hasActivity: jobRows > 0 || !!timer || stopwatch || missionActive,
        hasFocus: focus.active,
        thinking: agentThinking,
        ttsSpeaking,
        missionActive: mode === "peek" && missionActive,
        peekDeck: st.peekDeck,
        focusBanner: mode === "peek" && showFocusBanner,
        jobRows,
        clockRows,
        voiceClock,
        lip: st.topCovered,
        confirm: confirmPending,
        filesH,
        clipsH,
      }),
    [
      mode,
      notchGeometry,
      withMedia,
      jobRows,
      clockRows,
      voiceClock,
      st.topCovered,
      st.peekDeck,
      showFocusBanner,
      focus.active,
      ttsSpeaking,
      missionActive,
      confirmPending,
      filesH,
      clipsH,
      !!timer,
      stopwatch,
      agentThinking,
    ],
  );

  const interactiveSpanW = dims.w;

  const lip = mode === "compact" && st.topCovered;
  const workspaceDims = dimsFor("workspace", {
    notch: notchGeometry,
    hasMedia: withMedia,
    hasActivity: jobRows > 0,
    jobRows,
    lip: false,
    confirm: confirmPending,
  });

  const spring = reduced ? REDUCED : SHAPE_SPRING;
  const w = useSpring(dims.w, spring);
  const h = useSpring(dims.h, spring);
  useEffect(() => {
    w.set(dims.w);
    h.set(dims.h);
    void ipc.island.setShape(shapeRect(dims, interactiveSpanW));
    return sampleFrames(`${mode} ${dims.w}x${dims.h}`);
  }, [dims, interactiveSpanW, mode, w, h]);

  useEffect(() => {
    if (mode === "peek") void refreshControls();
  }, [mode, refreshControls]);

  const r = useTransform(h, (hv) => bottomRadius(hv));
  const d = useTransform(() => notchPath(w.get(), h.get(), r.get(), notch.hasNotch ? 0 : shoulderFor(h.get())));
  const svgW = useTransform(() => w.get() + (notch.hasNotch ? 0 : shoulderFor(h.get()) * 2));
  const svgLeft = useTransform(h, (hv) => notch.hasNotch ? 0 : -shoulderFor(hv));

  // State-derived ambient bottom glow (zero generic gradients; strictly real state).
  const ambientGlow = useMemo(() => {
    // One source of light: JARVIS itself, a low amber line at the lower
    // rim while it talks or works. The cover of what plays tints only the
    // timeline — washing half the deck in it read as a smear (2026-11-21).
    if (lip || mode === "compact") return "none";
    if (ttsSpeaking || mode === "voice" || missionActive) {
      return "radial-gradient(46% 14% at 50% 100%, rgba(255, 178, 63, 0.16) 0%, rgba(255, 178, 63, 0.04) 60%, transparent 100%)";
    }
    return "none";
  }, [lip, mode, ttsSpeaking, missionActive]);

  // Gaze follows the pointer while it is over the island.
  const lookX = useSpring(0, { stiffness: 200, damping: 24 });
  const onPointerMove = (e: ReactPointerEvent) => {
    const cx = window.innerWidth / 2 || CANVAS_W / 2;
    lookX.set(Math.max(-1, Math.min(1, (e.clientX - cx) / Math.max(80, dims.w / 2))));
  };
  const zero = useMotionValue(0);

  return (
    <div
      className={`stage${notch.hasNotch ? " has-notch" : ""}${st.visible ? "" : " is-hidden"}${withMedia && media?.status === "playing" ? " is-music" : ""}`}
      style={{ ["--art" as string]: artworkRgb ?? "236, 235, 230", ["--notch-width" as string]: `${notch.exclusion}px`, ["--compact-height" as string]: `${dims.h}px` }}
    >
      <motion.div
        className="island"
        style={{ width: w, height: h }}
        onPointerMove={onPointerMove}
        onPointerLeave={() => lookX.set(0)}
        onMouseEnter={() => {
          if (!isTauri()) st.set({ hovered: true });
        }}
        onMouseLeave={() => {
          if (!isTauri() && !st.inputFocused && !st.forceFocusBanner) {
            // In standalone browser QA, keep pinned decks open if QA harness pinned them;
            // natural hover works seamlessly when testing interactive states.
          }
        }}
      >
        <motion.svg className="island-shape" style={{ width: svgW, height: h, left: svgLeft }} aria-hidden="true">
          <defs>
            <linearGradient id="rim" x1="0" y1="0" x2="0" y2="1">
              <stop offset="0" stopColor="#ffffff" stopOpacity="0" />
              <stop offset="0.55" stopColor="#ffffff" stopOpacity="0.045" />
              <stop offset="1" stopColor="#ffffff" stopOpacity="0.15" />
            </linearGradient>
            <radialGradient id="sheen" cx="0.5" cy="0" r="0.95">
              <stop offset="0" stopColor="#0e0e12" />
              <stop offset="0.55" stopColor="#070709" />
              <stop offset="1" stopColor="#040406" />
            </radialGradient>
          </defs>
          <motion.path d={d} fill={notch.hasNotch ? "#000" : "url(#sheen)"} stroke={notch.hasNotch ? "none" : "url(#rim)"} strokeWidth={1} />
        </motion.svg>

        <motion.div className="island-clip" style={{ top: mode === "compact" ? 0 : notch.top, borderBottomLeftRadius: r, borderBottomRightRadius: r }}>
          <div className="island-ambient" style={{ background: ambientGlow }} aria-hidden="true" />

          <AnimatePresence initial={false} mode="popLayout">
            {mode !== "workspace" && (
              <motion.div
                key={mode}
                className="island-content"
                style={{ width: dims.w, height: dims.h - (mode === "compact" ? 0 : notch.top) }}
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
                {mode === "weather" && <WeatherView />}
                {mode === "clips" && <ClipsView />}
                {mode === "hello" && (helloCard ? <HeadsetHello /> : <UpdateCard />)}
              </motion.div>
            )}
          </AnimatePresence>

          {(workspaceWarm || mode === "workspace") && (
            <motion.div
              className="island-content"
              style={{
                width: workspaceDims.w,
                height: workspaceDims.h - notch.top,
                visibility: mode === "workspace" ? "visible" : "hidden",
                pointerEvents: mode === "workspace" ? "auto" : "none",
              }}
              aria-hidden={mode !== "workspace"}
              inert={mode !== "workspace"}
              initial={false}
              animate={{ opacity: mode === "workspace" ? 1 : 0 }}
              transition={{ duration: reduced ? 0 : 0.12 }}
            >
              <WorkspaceView visible={mode === "workspace"} />
            </motion.div>
          )}
        </motion.div>

        <div className="island-eyes" style={{ height: lip || (mode === "compact" && notch.compactHeight) ? dims.h : BROW_H }}>
          <Eyes
            mood={mode === "voice" ? voiceMood(voiceState, agentMood) : moodFor(mode, agentMood, st.hovered, ttsSpeaking)}
            level={voiceLevel}
            lookX={mode === "compact" ? zero : lookX}
            lookY={mode === "drop" ? 2 : 0}
            scale={lip ? 0.36 : 1}
            emote={currentEmote}
            emoteSeq={emoteSeq}
            bob={media?.status === "playing" && !ttsSpeaking}
            sleepy={night}
            watch={timerFinale && (mode === "compact" || mode === "peek")}
            alarm={mode === "notice"}
            activity={activity}
            onBoop={lip ? undefined : () => emote("boop")}
          />
        </div>
      </motion.div>
    </div>
  );
}
