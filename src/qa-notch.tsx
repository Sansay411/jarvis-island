// Visual QA harness for the JARVIS Dynamic Island (`qa/notch-showcase.html`).
// Renders the real `Island` component at y = 0 against a realistic desktop
// backdrop and exposes bottom state-switcher controls so every NotchNook,
// Needt, Reigne, TTS, and File-Drop state can be inspected live in Chrome.
// Sample state lives strictly in this QA harness — zero fake data in `src/island/**`.

import { StrictMode, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import "@fontsource-variable/onest";
import "@fontsource-variable/jetbrains-mono";
import "./styles/tokens.css";
import "./styles/base.css";

import { Island } from "./island/Island";
import { useIsland } from "./store/island";
import { useSystem } from "./store/system";
import { EMPTY_FOCUS, localDateIso, useHub } from "./store/hub";
import { useTts } from "./store/tts";
import { useVoice, outputLevel } from "./store/voice";
import { useFiles } from "./store/files";
import type { ActivityDay, ActivityHeatmap, ShelfItem, TaskItem } from "./lib/types";

type PresetId =
  | "compact"
  | "compact-live"
  | "nook"
  | "focus-banner"
  | "focus-deck"
  | "tray"
  | "activity"
  | "tts"
  | "voice"
  | "drop"
  | "files";

function makeCoverArtDataUrl(hue: "ocean" | "crimson" | "gold"): string {
  const c = document.createElement("canvas");
  c.width = 128;
  c.height = 128;
  const ctx = c.getContext("2d");
  if (!ctx) return "";
  const g = ctx.createRadialGradient(64, 42, 8, 64, 70, 78);
  if (hue === "ocean") {
    g.addColorStop(0, "#3b9eff");
    g.addColorStop(0.55, "#0d4a8a");
    g.addColorStop(1, "#061326");
  } else if (hue === "crimson") {
    g.addColorStop(0, "#ff5e7a");
    g.addColorStop(0.55, "#9c1f3d");
    g.addColorStop(1, "#1a070d");
  } else {
    g.addColorStop(0, "#ffd257");
    g.addColorStop(0.55, "#b86b11");
    g.addColorStop(1, "#1c1004");
  }
  ctx.fillStyle = g;
  ctx.fillRect(0, 0, 128, 128);

  // Subtle horizon silhouette echoing Ref 2 cover art.
  ctx.fillStyle = "rgba(4, 6, 10, 0.78)";
  ctx.beginPath();
  ctx.arc(58, 74, 15, 0, Math.PI * 2);
  ctx.fill();
  ctx.beginPath();
  ctx.ellipse(58, 122, 30, 34, 0, Math.PI, 0);
  ctx.fill();
  return c.toDataURL("image/png");
}

function buildSampleTasks(): TaskItem[] {
  const today = localDateIso();
  const nowIso = new Date().toISOString();
  return [
    {
      id: "qa-t1",
      title: "Подготовить еженедельный отчёт",
      notes: "",
      scheduledDate: today,
      scheduledTime: "15:00",
      color: "blue",
      completed: false,
      completedAt: null,
      createdAt: nowIso,
    },
    {
      id: "qa-t2",
      title: "Созвон по архитектуре TTS",
      notes: "",
      scheduledDate: today,
      scheduledTime: "16:30",
      color: "amber",
      completed: false,
      completedAt: null,
      createdAt: nowIso,
    },
    {
      id: "qa-t3",
      title: "Тренировка в зале",
      notes: "",
      scheduledDate: today,
      scheduledTime: "18:15",
      color: "coral",
      completed: false,
      completedAt: null,
      createdAt: nowIso,
    },
    {
      id: "qa-t4",
      title: "Ревью ветки feat/agent-control",
      notes: "",
      scheduledDate: today,
      scheduledTime: "20:00",
      color: "emerald",
      completed: false,
      completedAt: null,
      createdAt: nowIso,
    },
  ];
}

function buildSampleShelf(): ShelfItem[] {
  const nowIso = new Date().toISOString();
  return [
    {
      id: "qa-s1",
      kind: "file",
      label: "JARVIS-Architecture-2026.pdf",
      value: "C:\\Users\\-pin\\Documents\\Jarvis\\docs\\JARVIS-Architecture-2026.pdf",
      subtitle: "docs\\JARVIS-Architecture-2026.pdf",
      sizeBytes: 2_485_120,
      exists: true,
      pinned: true,
      createdAt: nowIso,
    },
    {
      id: "qa-s2",
      kind: "text",
      label: "Проверить задержку первого аудио-чанка Edge Neural (< 220 мс)",
      value: "Проверить задержку первого аудио-чанка Edge Neural (< 220 мс) и кэш SHA-256.",
      subtitle: "76 симв.",
      sizeBytes: null,
      exists: true,
      pinned: false,
      createdAt: nowIso,
    },
    {
      id: "qa-s3",
      kind: "url",
      label: "tauri.app/v2/reference/javascript/api",
      value: "https://tauri.app/v2/reference/javascript/api",
      subtitle: "Веб-ссылка",
      sizeBytes: null,
      exists: true,
      pinned: false,
      createdAt: nowIso,
    },
  ];
}

function buildSampleHeatmap(): ActivityHeatmap {
  const today = new Date();
  const base = new Date(today.getFullYear(), today.getMonth(), today.getDate());
  const days: ActivityDay[] = [];
  let total = 0;

  for (let i = 363; i >= 0; i--) {
    const dt = new Date(base.getFullYear(), base.getMonth(), base.getDate() - i);
    const iso = localDateIso(dt);
    // Deterministic realistic cadence with recent 6-day active streak.
    const seed = (i * 37 + 13) % 19;
    const inStreak = i < 6;
    const active = inStreak || (seed > 5 && dt.getDay() !== 0);
    const commits = active ? (inStreak ? 4 + (i % 5) : seed % 7) : 0;
    const focusMin = active ? (inStreak ? 50 : (seed % 3) * 25) : 0;
    const tasksDone = active ? 1 + (seed % 4) : 0;
    const agentActs = active && seed % 2 === 0 ? 2 + (seed % 4) : 0;
    const count = commits + Math.round(focusMin / 25) + tasksDone + agentActs;
    total += count;
    const level: 0 | 1 | 2 | 3 | 4 =
      count === 0 ? 0 : count <= 4 ? 1 : count <= 8 ? 2 : count <= 13 ? 3 : 4;
    days.push({
      date: iso,
      count,
      level,
      focusMinutes: focusMin,
      tasksDone,
      agentActions: agentActs,
      gitCommits: commits,
    });
  }

  return {
    userName: "Данила",
    totalYear: total,
    currentStreak: 6,
    longestStreak: 14,
    days,
  };
}

const PRESETS: Array<{ id: PresetId; label: string }> = [
  { id: "compact", label: "1. Compact (Покой)" },
  { id: "compact-live", label: "2. Compact + Медиа + Фокус" },
  { id: "nook", label: "3. Обзор: Плеер + Календарь (NotchNook)" },
  { id: "focus-banner", label: "4. Hover: Фокус-баннер (Needt)" },
  { id: "focus-deck", label: "5. Deck: Фокус и задачи (Needt)" },
  { id: "tray", label: "6. Deck: Полка (Tray)" },
  { id: "activity", label: "7. Deck: Стрик (Reigne)" },
  { id: "tts", label: "8. Озвучка TTS (Живые глаза)" },
  { id: "voice", label: "9. Голосовой диалог" },
  { id: "drop", label: "10. Сброс файлов (Drop)" },
  { id: "files", label: "11. Действия с файлом (+ На полку)" },
];

const SOURCES: Array<{ id: string; label: string; app: string; track: string; artist: string; hue: "ocean" | "crimson" | "gold" }> = [
  { id: "spotify", label: "Spotify", app: "Spotify.exe", track: "Stargazing", artist: "Myles Smith", hue: "ocean" },
  { id: "apple", label: "Apple Music", app: "AppleMusic.exe", track: "Cocoon", artist: "Björk · Vespertine", hue: "crimson" },
  { id: "yandex", label: "Яндекс Музыка", app: "YandexMusic.exe", track: "Ночной полёт", artist: "Антон Беляев", hue: "gold" },
];

function QaHarness() {
  const [activePreset, setActivePreset] = useState<PresetId>("nook");
  const [sourceIdx, setSourceIdx] = useState<number>(0);

  const seedMedia = (idx: number, playing = true) => {
    const src = SOURCES[idx % SOURCES.length]!;
    const key = `qa-track-${src.id}`;
    useSystem.setState({
      audio: { volume: 68, muted: false, device: "Динамики (Realtek Audio)" },
      brightness: [{ id: "internal", name: "Встроенный дисплей", kind: "internal", brightness: 82, supported: true }],
      media: {
        available: true,
        sourceApp: src.app,
        title: src.track,
        artist: src.artist,
        album: src.track,
        status: playing ? "playing" : "paused",
        positionMs: 102_000,
        durationMs: 174_000,
        positionSampledAt: Date.now(),
        canToggle: true,
        canNext: true,
        canPrevious: true,
        trackKey: key,
      },
      artwork: {
        trackKey: key,
        dataUrl: makeCoverArtDataUrl(src.hue),
      },
    });
  };

  const applyPreset = (id: PresetId, srcIndex = sourceIdx) => {
    setActivePreset(id);

    // Reset transient modal flags first.
    void useTts.getState().stop();
    outputLevel.set(0);
    useVoice.setState({ state: "idle", error: null, userLine: "", assistantLine: "" });
    useFiles.setState({ open: false, loading: false, error: null, paths: [], files: [], actions: [], job: null });

    // Seed baseline Hub data for realistic inspection.
    useHub.setState({
      tasks: buildSampleTasks(),
      shelf: buildSampleShelf(),
      activity: buildSampleHeatmap(),
      missionSteps: [],
      fullControl: id === "focus-deck" || id === "activity",
    });

    const activeFocus = {
      active: true,
      paused: false,
      taskId: "qa-t1",
      title: "Глубокая работа над главным проектом",
      totalSec: 25 * 60,
      remainingSec: 17 * 60 + 55,
      startedAt: new Date(Date.now() - 7 * 60 * 1000).toISOString(),
      endsAt: new Date(Date.now() + (17 * 60 + 55) * 1000).toISOString(),
      completedToday: 3,
      minutesToday: 75,
    };

    switch (id) {
      case "compact":
        useSystem.setState({ media: null, artwork: null });
        useHub.setState({ focus: EMPTY_FOCUS });
        useIsland.setState({
          hovered: false,
          dragOver: false,
          voiceOpen: false,
          workspaceOpen: false,
          clipsOpen: false,
          forceFocusBanner: false,
          focusBannerDismissed: false,
        });
        break;

      case "compact-live":
        seedMedia(srcIndex, true);
        useHub.setState({ focus: activeFocus });
        useIsland.setState({
          hovered: false,
          dragOver: false,
          voiceOpen: false,
          workspaceOpen: false,
          clipsOpen: false,
          forceFocusBanner: false,
          focusBannerDismissed: false,
        });
        break;

      case "nook":
        seedMedia(srcIndex, true);
        useHub.setState({ focus: EMPTY_FOCUS });
        useIsland.setState({
          hovered: true,
          peekDeck: "nook",
          dragOver: false,
          voiceOpen: false,
          workspaceOpen: false,
          clipsOpen: false,
          forceFocusBanner: false,
          focusBannerDismissed: true,
        });
        break;

      case "focus-banner":
        seedMedia(srcIndex, true);
        useHub.setState({ focus: activeFocus });
        useIsland.setState({
          hovered: true,
          peekDeck: "focus",
          dragOver: false,
          voiceOpen: false,
          workspaceOpen: false,
          clipsOpen: false,
          forceFocusBanner: true,
          focusBannerDismissed: false,
        });
        break;

      case "focus-deck":
        seedMedia(srcIndex, true);
        useHub.setState({ focus: activeFocus });
        useIsland.setState({
          hovered: true,
          peekDeck: "focus",
          dragOver: false,
          voiceOpen: false,
          workspaceOpen: false,
          clipsOpen: false,
          forceFocusBanner: false,
          focusBannerDismissed: true,
        });
        break;

      case "tray":
        seedMedia(srcIndex, true);
        useHub.setState({ focus: EMPTY_FOCUS });
        useIsland.setState({
          hovered: true,
          peekDeck: "tray",
          dragOver: false,
          voiceOpen: false,
          workspaceOpen: false,
          clipsOpen: false,
          forceFocusBanner: false,
          focusBannerDismissed: true,
        });
        break;

      case "activity":
        seedMedia(srcIndex, true);
        useHub.setState({ focus: EMPTY_FOCUS });
        useIsland.setState({
          hovered: true,
          peekDeck: "activity",
          dragOver: false,
          voiceOpen: false,
          workspaceOpen: false,
          clipsOpen: false,
          forceFocusBanner: false,
          focusBannerDismissed: true,
        });
        break;

      case "tts":
        seedMedia(srcIndex, false);
        useHub.setState({
          focus: EMPTY_FOCUS,
          missionSteps: [
            { id: "s1", title: "Собрать телеметрию окон и процессов", status: "done" },
            { id: "s2", title: "Озвучить утренний брифинг вслух", status: "running" },
            { id: "s3", title: "Запустить сессию глубокого фокуса", status: "pending" },
          ],
        });
        useIsland.setState({
          hovered: true,
          peekDeck: "nook",
          dragOver: false,
          voiceOpen: false,
          workspaceOpen: false,
          clipsOpen: false,
          forceFocusBanner: false,
          focusBannerDismissed: true,
        });
        void useTts
          .getState()
          .speak("Доброе утро, Данила. На сегодня запланировано четыре задачи, всё готово к глубокому фокусу.");
        break;

      case "voice":
        useVoice.setState({
          state: "speaking",
          error: null,
          userLine: "Джарвис, включи режим фокуса на двадцать пять минут",
          assistantLine: "Запускаю глубокий фокус на двадцать пять минут и приглушаю лишние окна.",
        });
        outputLevel.set(0.62);
        useIsland.setState({
          hovered: false,
          dragOver: false,
          voiceOpen: true,
          workspaceOpen: false,
          clipsOpen: false,
          forceFocusBanner: false,
        });
        break;

      case "drop":
        useIsland.setState({
          hovered: true,
          dragOver: true,
          voiceOpen: false,
          workspaceOpen: false,
          clipsOpen: false,
          forceFocusBanner: false,
        });
        break;

      case "files":
        useFiles.setState({
          open: true,
          loading: false,
          error: null,
          paths: ["C:\\Users\\-pin\\Documents\\Spec-2026.pdf"],
          files: [
            {
              path: "C:\\Users\\-pin\\Documents\\Spec-2026.pdf",
              name: "Spec-2026.pdf",
              ext: "pdf",
              kind: "pdf",
              size: 1_842_300,
              width: null,
              height: null,
              durationS: null,
              pages: 18,
              videoCodec: null,
              audioCodec: null,
              thumbnail: null,
            },
          ],
          actions: [
            { id: "ask:summarize", label: "Кратко пересказать", hint: null, primary: true },
            { id: "chat", label: "В чат", hint: null, primary: true },
            { id: "open", label: "Открыть", hint: null, primary: false },
            { id: "reveal", label: "Показать в папке", hint: null, primary: false },
          ],
          job: null,
          height: 210,
        });
        useIsland.setState({
          hovered: true,
          dragOver: false,
          voiceOpen: false,
          workspaceOpen: false,
          clipsOpen: false,
          forceFocusBanner: false,
        });
        break;
    }
  };

  useEffect(() => {
    document.body.classList.add("island-window");
    const params = new URLSearchParams(window.location.search);
    const srcParam = params.get("source");
    const foundSrc = srcParam ? SOURCES.findIndex((s) => s.id === srcParam) : -1;
    const initialSrc = foundSrc >= 0 ? foundSrc : 0;
    if (foundSrc >= 0) setSourceIdx(foundSrc);

    const presetParam = params.get("preset") as PresetId | null;
    const valid = PRESETS.some((p) => p.id === presetParam) ? presetParam! : "nook";
    applyPreset(valid, initialSrc);
  }, []);

  const cycleSource = (idx: number) => {
    setSourceIdx(idx);
    seedMedia(idx, true);
  };

  return (
    <div style={styles.wallpaper}>
      {/* Simulated physical monitor top bezel line at y = 0 */}
      <div style={styles.bezelHairline} aria-hidden="true" />

      {/* Real Dynamic Island component */}
      <Island />

      {/* Visual QA State Switcher Dock at bottom */}
      <div style={styles.dockWrap}>
        <div style={styles.dockHeader}>
          <span style={styles.dockTitle}>JARVIS Dynamic Island · Visual QA</span>
          <div style={styles.sourceGroup}>
            <span style={styles.sourceHint}>Источник медиа:</span>
            {SOURCES.map((s, idx) => (
              <button
                key={s.id}
                type="button"
                onClick={() => cycleSource(idx)}
                style={{
                  ...styles.sourceChip,
                  ...(sourceIdx === idx ? styles.sourceChipActive : null),
                }}
              >
                {s.label}
              </button>
            ))}
            <button
              type="button"
              onClick={() => useSystem.setState({ media: null, artwork: null })}
              style={styles.sourceChip}
            >
              Без музыки
            </button>
          </div>
        </div>

        <div style={styles.presetGrid}>
          {PRESETS.map((p) => {
            const active = activePreset === p.id;
            return (
              <button
                key={p.id}
                type="button"
                data-preset={p.id}
                onClick={() => applyPreset(p.id)}
                style={{
                  ...styles.presetBtn,
                  ...(active ? styles.presetBtnActive : null),
                }}
              >
                {p.label}
              </button>
            );
          })}
        </div>
      </div>
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  wallpaper: {
    position: "fixed",
    inset: 0,
    background:
      "radial-gradient(120% 80% at 50% 0%, #243242 0%, #161f2b 48%, #0d1219 100%)",
    overflow: "hidden",
  },
  bezelHairline: {
    position: "fixed",
    top: 0,
    left: 0,
    right: 0,
    height: 1,
    background: "rgba(0, 0, 0, 0.85)",
    pointerEvents: "none",
  },
  dockWrap: {
    position: "fixed",
    left: "50%",
    bottom: 18,
    transform: "translateX(-50%)",
    width: "min(94vw, 960px)",
    padding: "12px 14px",
    borderRadius: 16,
    background: "rgba(10, 11, 15, 0.9)",
    boxShadow: "0 16px 40px rgba(0,0,0,0.5), inset 0 0 0 1px rgba(255,255,255,0.08)",
    display: "flex",
    flexDirection: "column",
    gap: 10,
    zIndex: 50,
  },
  dockHeader: {
    display: "flex",
    alignItems: "center",
    justifyContent: "space-between",
    flexWrap: "wrap",
    gap: 8,
  },
  dockTitle: {
    fontSize: 12,
    fontWeight: 650,
    color: "#f2f2f0",
    letterSpacing: "0.01em",
  },
  sourceGroup: {
    display: "flex",
    alignItems: "center",
    flexWrap: "wrap",
    gap: 5,
  },
  sourceHint: {
    fontSize: 11,
    color: "#7c7c86",
    marginRight: 2,
  },
  sourceChip: {
    height: 23,
    padding: "0 9px",
    borderRadius: 999,
    background: "#181920",
    boxShadow: "inset 0 0 0 1px rgba(255,255,255,0.08)",
    color: "#a4a4aa",
    fontSize: 11,
    fontWeight: 560,
  },
  sourceChipActive: {
    background: "#262834",
    color: "#ffffff",
    boxShadow: "inset 0 0 0 1px rgba(255,255,255,0.22)",
  },
  presetGrid: {
    display: "flex",
    flexWrap: "wrap",
    gap: 6,
  },
  presetBtn: {
    height: 28,
    padding: "0 11px",
    borderRadius: 9,
    background: "#15161c",
    boxShadow: "inset 0 0 0 1px rgba(255,255,255,0.07)",
    color: "#b4b4bc",
    fontSize: 11.5,
    fontWeight: 560,
  },
  presetBtnActive: {
    background: "#0a84ff",
    color: "#ffffff",
    boxShadow: "0 2px 10px rgba(10, 132, 255, 0.35)",
  },
};

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <QaHarness />
  </StrictMode>,
);
