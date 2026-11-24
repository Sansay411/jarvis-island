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
import { useGithub } from "./store/github";
import { useAgent } from "./store/agent";
import { useWeather, type Weather } from "./store/weather";
import { primeShelfThumbs } from "./island/shelfThumbs";
import { useBluetooth } from "./store/bluetooth";
import { useMentor } from "./store/mentor";
import { useAgenda, remember, upcoming, type CalEvent } from "./store/agenda";
import { usePrefs } from "./store/prefs";
import { ACTIVITIES, emote as triggerEmote, type Activity, type Emote } from "./store/emote";
import { SKINS, normalizeSkin, type SkinId } from "./island/skins/types";
import { isoDateTime } from "./planner/dates";
import type { BtDevice } from "./lib/types";

const SAMPLE_WEATHER: Weather = {
  place: { city: "Караганда", region: "Карагандинская область", country: "Казахстан", lat: 49.8, lon: 73.1, approximate: true },
  temp: -1.3,
  feels: -5.2,
  kind: "clear",
  text: "Ясно",
  isDay: false,
  wind: 2.2,
  humidity: 81,
  hours: [
    { time: "19:00", temp: -2, kind: "clear", precip: 0 },
    { time: "21:00", temp: -3, kind: "clear", precip: 0 },
    { time: "23:00", temp: -4, kind: "partly", precip: 0 },
    { time: "01:00", temp: -5, kind: "cloud", precip: 10 },
    { time: "03:00", temp: -5, kind: "snow", precip: 40 },
    { time: "05:00", temp: -6, kind: "snow", precip: 60 },
  ],
  today: { max: 3, min: -6, kind: "clear", text: "Ясно", precip: 0, wind: 4, sunrise: "08:01", sunset: "16:40" },
  tomorrow: { max: 4, min: -5, kind: "cloud", text: "Пасмурно", precip: 20, wind: 7, sunrise: "08:03", sunset: "16:39" },
  tomorrowHours: [
    { time: "06:00", temp: -5, kind: "cloud", precip: 0 },
    { time: "09:00", temp: -3, kind: "cloud", precip: 10 },
    { time: "12:00", temp: 2, kind: "partly", precip: 10 },
    { time: "15:00", temp: 4, kind: "cloud", precip: 20 },
    { time: "18:00", temp: 0, kind: "cloud", precip: 10 },
    { time: "21:00", temp: -2, kind: "cloud", precip: 0 },
  ],
};
import type { GithubState } from "./lib/types";
import type { ActivityDay, ActivityHeatmap, ShelfItem, TaskItem } from "./lib/types";

type PresetId =
  | "compact"
  | "compact-live"
  | "nook"
  | "focus-banner"
  | "focus-deck"
  | "tray"
  | "tray-empty"
  | "activity"
  | "github-connect"
  | "thinking"
  | "weather"
  | "weather-tomorrow"
  | "empty-nook"
  | "empty-focus"
  | "tts"
  | "voice"
  | "drop"
  | "drop-send"
  | "files"
  | "sound"
  | "sound-single"
  | "sound-low"
  | "sound-empty"
  | "hello"
  | "pinned"
  | "growth"
  | "watching"
  | "nook-events"
  | "event-soon"
  | "event-now"
  | "compact-soon"
  | "gain"
  | "moment-day"
  | "moment-level"
  | "growth-card"
  | "focus-cal"
  | "skin-amber"
  | "skin-orb"
  | "skin-ice"
  | "skin-pixel"
  | "skin-minimal";

const SAMPLE_TOOL_FOR_ACTIVITY: Record<Activity, { tool: string; title: string }> = {
  search: { tool: "web_search", title: "Ищу в интернете" },
  read: { tool: "file_read", title: "Читаю документ" },
  type: { tool: "input_type", title: "Печатаю текст" },
  click: { tool: "input_click", title: "Нажимаю кнопку" },
  look: { tool: "screen_capture", title: "Смотрю на экран" },
  draw: { tool: "screen_point", title: "Показываю на экране" },
  files: { tool: "files_search", title: "Ищу файлы" },
  music: { tool: "media_control", title: "Переключаю трек" },
  calendar: { tool: "calendar_manage", title: "Сверяю календарь" },
  memory: { tool: "memory_save", title: "Запоминаю факт" },
  time: { tool: "reminder_create", title: "Ставлю таймер" },
  system: { tool: "system_volume", title: "Настраиваю систему" },
  code: { tool: "shell_run", title: "Выполняю команду" },
  connect: { tool: "mcp__github__list_issues", title: "Запрос к MCP" },
  speak: { tool: "speak_aloud", title: "Озвучиваю ответ" },
  work: { tool: "custom_unknown_tool", title: "Выполняю задачу" },
};

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

/** Calendar blocks around «now» so the overview shows past, now and next. */
function buildSampleEvents(variant: "day" | "soon" | "now"): CalEvent[] {
  const base = new Date();
  base.setSeconds(0, 0);
  const at = (min: number) => isoDateTime(new Date(base.getTime() + min * 60_000));
  const today = localDateIso(base);
  const tomorrow = localDateIso(new Date(base.getTime() + 86_400_000));
  let n = 1;
  const ev = (start: string, end: string, title: string, category: string, extra: Partial<CalEvent> = {}): CalEvent => ({
    id: `qa-ev-${n++}`,
    title,
    notes: "",
    location: "",
    start,
    end,
    allDay: start.length === 10,
    color: "",
    category,
    done: false,
    seriesId: null,
    googleId: null,
    recurring: false,
    updatedAt: base.toISOString(),
    ...extra,
  });
  if (variant === "soon") return [ev(at(6), at(66), "Встреча с заказчиком", "work", { location: "Zoom" })];
  if (variant === "now") return [ev(at(-1), at(89), "Глубокая работа: jarvis-island", "project")];
  return [
    ev(today, tomorrow, "Неделя без коротких видео", "study"),
    ev(at(-190), at(-170), "Стендап", "work", { done: true }),
    ev(at(-35), at(25), "Глубокая работа: jarvis-island", "project"),
    ev(at(40), at(85), "Созвон по архитектуре TTS", "work", { location: "Zoom" }),
    ev(at(180), at(240), "Зал: ноги", "sport"),
  ];
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

/** QA-only preview pictures for shelf tiles (canvas gradients). */
function qaThumb(a: string, b: string, label: string): string {
  const c = document.createElement("canvas");
  c.width = 192;
  c.height = 192;
  const g = c.getContext("2d")!;
  const grad = g.createLinearGradient(0, 0, 192, 192);
  grad.addColorStop(0, a);
  grad.addColorStop(1, b);
  g.fillStyle = grad;
  g.fillRect(0, 0, 192, 192);
  g.fillStyle = "rgba(255,255,255,0.85)";
  g.font = "600 22px sans-serif";
  g.fillText(label, 18, 44);
  return c.toDataURL("image/png");
}

function buildSampleShelf(): ShelfItem[] {
  const nowIso = new Date().toISOString();
  primeShelfThumbs({
    "C:\\Users\\-pin\\Pictures\\Rocks.png": qaThumb("#6aa7d8", "#2c4a63", "Tahoe"),
    "C:\\Users\\-pin\\Videos\\demo-island.mp4": qaThumb("#3b2a1c", "#0e0e10", "▶ 0:22"),
  });
  return [
    {
      id: "qa-s0",
      kind: "file",
      label: "Rocks.png",
      value: "C:\\Users\\-pin\\Pictures\\Rocks.png",
      subtitle: "Pictures\\Rocks.png",
      sizeBytes: 1_204_000,
      exists: true,
      pinned: false,
      createdAt: nowIso,
    },
    {
      id: "qa-s4",
      kind: "file",
      label: "demo-island.mp4",
      value: "C:\\Users\\-pin\\Videos\\demo-island.mp4",
      subtitle: "Videos\\demo-island.mp4",
      sizeBytes: 9_400_000,
      exists: true,
      pinned: false,
      createdAt: nowIso,
    },
    {
      id: "qa-s5",
      kind: "file",
      label: "Отчёт_за_ноябрь_финальный.pdf",
      value: "C:\\Users\\-pin\\Desktop\\Отчёт_за_ноябрь_финальный.pdf",
      subtitle: "Desktop",
      sizeBytes: 320_000,
      exists: false,
      pinned: false,
      createdAt: nowIso,
    },
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

function buildSampleGithub(): GithubState {
  const today = new Date();
  const days = [];
  let total = 0;
  for (let i = 370; i >= 0; i--) {
    const dt = new Date(today.getFullYear(), today.getMonth(), today.getDate() - i);
    const seed = (i * 37 + 13) % 19;
    const active = i < 6 || (seed > 6 && dt.getDay() !== 0 && dt.getDay() !== 6);
    const count = active ? (i < 6 ? 3 + (i % 4) : seed % 9) : 0;
    total += count;
    days.push({ date: localDateIso(dt), count, level: count === 0 ? 0 : count <= 2 ? 1 : count <= 4 ? 2 : count <= 6 ? 3 : 4 });
  }
  const c = document.createElement("canvas");
  c.width = c.height = 64;
  const ctx = c.getContext("2d");
  if (ctx) {
    const g = ctx.createLinearGradient(0, 0, 64, 64);
    g.addColorStop(0, "#3a3f4a");
    g.addColorStop(1, "#14161b");
    ctx.fillStyle = g;
    ctx.fillRect(0, 0, 64, 64);
    ctx.fillStyle = "#c9ccd3";
    ctx.beginPath();
    ctx.arc(32, 25, 11, 0, Math.PI * 2);
    ctx.fill();
    ctx.beginPath();
    ctx.ellipse(32, 60, 21, 17, 0, 0, Math.PI * 2);
    ctx.fill();
  }
  return {
    login: "danila-dev",
    activity: {
      login: "danila-dev",
      name: "Данила",
      avatar: c.toDataURL("image/png"),
      profileUrl: "https://github.com/danila-dev",
      total,
      currentStreak: 6,
      longestStreak: 14,
      days,
      source: "gh",
      fetchedAt: new Date().toISOString(),
    },
    error: null,
  };
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
  { id: "tray-empty", label: "6b. Полка пустая" },
  { id: "activity", label: "7. Deck: Стрик (GitHub)" },
  { id: "github-connect", label: "7b. Стрик: подключить GitHub" },
  { id: "thinking", label: "2b. Думает (компакт)" },
  { id: "weather", label: "12. Погода (карточка)" },
  { id: "weather-tomorrow", label: "12б. Погода на завтра" },
  { id: "empty-nook", label: "3b. Обзор: без музыки и задач" },
  { id: "empty-focus", label: "5b. Фокус: без задач" },
  { id: "tts", label: "8. Озвучка TTS (Живые глаза)" },
  { id: "voice", label: "9. Голосовой диалог" },
  { id: "drop", label: "10. Сброс файлов (Drop)" },
  { id: "drop-send", label: "10b. Сброс над «В Telegram»" },
  { id: "files", label: "11. Действия с файлом (+ На полку)" },
  { id: "sound", label: "13. Звук: AirPods (каждый наушник и кейс)" },
  { id: "sound-single", label: "13b. Звук: накладные наушники" },
  { id: "sound-low", label: "13c. Звук: садятся" },
  { id: "sound-empty", label: "13d. Звук: ничего не подключено" },
  { id: "hello", label: "14. Наушники подключились" },
  { id: "pinned", label: "15. Закреплённый остров" },
  { id: "growth", label: "16. Рост (наставник)" },
  { id: "watching", label: "17. Компакт: минуты TikTok" },
  { id: "nook-events", label: "18. Обзор: события календаря" },
  { id: "event-soon", label: "19. Событие через 6 мин" },
  { id: "event-now", label: "19b. Событие начинается" },
  { id: "compact-soon", label: "19c. Компакт: событие скоро" },
  { id: "gain", label: "20. Компакт: +XP" },
  { id: "moment-day", label: "21. День засчитан" },
  { id: "moment-level", label: "21b. Новый уровень" },
  { id: "growth-card", label: "22. Рост: карточка на острове" },
  { id: "focus-cal", label: "23. Фокус: блок из календаря" },
  { id: "skin-amber", label: "Облик: Янтарь" },
  { id: "skin-orb", label: "Облик: Сфера" },
  { id: "skin-ice", label: "Облик: Лёд" },
  { id: "skin-pixel", label: "Облик: Пиксель" },
  { id: "skin-minimal", label: "Облик: Минимал" },
];

const SOURCES: Array<{ id: string; label: string; app: string; track: string; artist: string; hue: "ocean" | "crimson" | "gold" }> = [
  { id: "spotify", label: "Spotify", app: "Spotify.exe", track: "Stargazing", artist: "Myles Smith", hue: "ocean" },
  { id: "apple", label: "Apple Music", app: "AppleMusic.exe", track: "Cocoon", artist: "Björk · Vespertine", hue: "crimson" },
  { id: "yandex", label: "Яндекс Музыка", app: "YandexMusic.exe", track: "Ночной полёт", artist: "Антон Беляев", hue: "gold" },
];

/** Battery samples every 10 minutes falling linearly from `from` to `to`. */
function qaHistory(from: number, to: number, hours: number): Array<[number, number]> {
  const now = Date.now();
  const steps = Math.max(2, Math.round((hours * 60) / 10));
  return Array.from({ length: steps + 1 }, (_, i) => [now - (hours * 3600_000 * (steps - i)) / steps, Math.round(from + ((to - from) * i) / steps)] as [number, number]);
}

function buildBtDevices(variant: "pods" | "single" | "low" | "away"): BtDevice[] {
  const now = Date.now();
  const base: Omit<BtDevice, "id" | "name" | "kind" | "connected" | "battery"> = {
    lidOpen: null,
    model: null,
    vendor: null,
    audioOutput: false,
    history: [],
    minutesLeft: null,
    drainPerHour: null,
    updatedAt: now - 20_000,
    left: null,
    right: null,
    case: null,
  };
  const pods: BtDevice = {
    ...base,
    id: "qa-pods",
    name: "AirPods Pro Артёма",
    kind: "earbuds",
    connected: true,
    battery: 80,
    left: { level: 90, charging: false, inEar: true },
    right: { level: 80, charging: false, inEar: true },
    case: { level: 40, charging: true, inEar: null },
    model: "AirPods Pro 2 (USB‑C)",
    vendor: "Apple",
    audioOutput: true,
    history: qaHistory(100, 80, 2.2),
    minutesLeft: 250,
    drainPerHour: 19,
  };
  const sony: BtDevice = {
    ...base,
    id: "qa-sony",
    name: "WH-1000XM5",
    kind: "headphones",
    connected: true,
    battery: variant === "low" ? 12 : 64,
    vendor: "Sony",
    audioOutput: true,
    history: variant === "low" ? qaHistory(46, 12, 5) : qaHistory(92, 64, 5),
    minutesLeft: variant === "low" ? 105 : 11 * 60 + 30,
    drainPerHour: variant === "low" ? 6.8 : 5.6,
  };
  const keyboard: BtDevice = { ...base, id: "qa-kb", name: "Keyboard K380", kind: "keyboard", connected: true, battery: 70, vendor: "Logitech" };
  const mouse: BtDevice = { ...base, id: "qa-mouse", name: "MX Master 3S", kind: "mouse", connected: true, battery: 15, vendor: "Logitech" };
  const phone: BtDevice = { ...base, id: "qa-phone", name: "iPhone", kind: "phone", connected: false, battery: 60, vendor: "Apple" };
  switch (variant) {
    case "pods":
      return [pods, keyboard, mouse, phone];
    case "single":
    case "low":
      return [sony, keyboard, phone];
    case "away":
      return [{ ...pods, connected: false, audioOutput: false, battery: 60, left: null, right: null, case: null, history: [] }, keyboard, phone];
  }
}

function QaHarness() {
  const [activePreset, setActivePreset] = useState<PresetId>("nook");
  const [sourceIdx, setSourceIdx] = useState<number>(0);
  const [activeSkin, setActiveSkin] = useState<SkinId>("amber");
  const [activeActivity, setActiveActivity] = useState<Activity | null>(null);

  const selectSkin = (skin: SkinId) => {
    setActiveSkin(skin);
    usePrefs.getState().setSkin(skin);
  };

  const selectActivity = (act: Activity | null) => {
    setActiveActivity(act);
    if (!act) {
      useAgent.setState({ activeTurn: null, phase: null, items: [] });
      return;
    }
    const sample = SAMPLE_TOOL_FOR_ACTIVITY[act];
    useAgent.setState({
      activeTurn: "qa-turn",
      phase: "acting",
      items: [
        {
          kind: "tool",
          id: `qa-tool-${act}`,
          turnId: "qa-turn",
          tool: sample.tool,
          title: sample.title,
          status: "running",
          args: {},
        },
      ],
    });
  };

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
    useBluetooth.setState({ devices: buildBtDevices(id === "sound-single" ? "single" : id === "sound-low" ? "low" : id === "sound-empty" ? "away" : "pods"), hello: null });
    useIsland.setState({ pinned: id === "pinned" });
    if (id !== "watching") useMentor.setState({ watching: null });
    useMentor.setState({ moment: null, gain: null });
    useIsland.setState({ growthCard: false });
    useAgenda.setState({ events: id === "focus-cal" || id === "nook-events" ? buildSampleEvents("day") : [], card: null, soon: null });

    // Seed baseline Hub data for realistic inspection.
    useHub.setState({
      tasks: buildSampleTasks(),
      shelf: id === "tray-empty" ? [] : buildSampleShelf(),
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

    useAgent.setState(
      id === "thinking"
        ? {
            activeTurn: "qa-turn",
            phase: "acting",
            items: [{ kind: "tool", id: "qa-tool", turnId: "qa-turn", tool: "web_search", title: "Ищу погоду на завтра", status: "running", args: {} }],
          }
        : { activeTurn: null, phase: null, items: [] },
    );
    setActiveActivity(id === "thinking" ? "search" : null);
    useWeather.setState({ card: id === "weather" ? SAMPLE_WEATHER : id === "weather-tomorrow" ? { ...SAMPLE_WEATHER, focus: "tomorrow" } : null, here: SAMPLE_WEATHER, loadedAt: Date.now() });
    useGithub.setState({ state: id === "github-connect" ? { login: null, activity: null, error: null } : buildSampleGithub(), loading: false, formError: null });

    switch (id) {
      case "weather":
      case "weather-tomorrow":
        useSystem.setState({ media: null, artwork: null });
        useHub.setState({ focus: EMPTY_FOCUS });
        useIsland.setState({ hovered: false, dragOver: false, voiceOpen: false, workspaceOpen: false, clipsOpen: false, forceFocusBanner: false, focusBannerDismissed: false });
        break;

      case "thinking":
        useSystem.setState({ media: null, artwork: null });
        useHub.setState({ focus: EMPTY_FOCUS });
        useIsland.setState({ hovered: false, dragOver: false, voiceOpen: false, workspaceOpen: false, clipsOpen: false, forceFocusBanner: false, focusBannerDismissed: false });
        break;

      case "github-connect":
        useSystem.setState({ media: null, artwork: null });
        useHub.setState({ focus: EMPTY_FOCUS });
        useIsland.setState({ hovered: true, peekDeck: "activity", dragOver: false, voiceOpen: false, workspaceOpen: false, clipsOpen: false, forceFocusBanner: false, focusBannerDismissed: true });
        break;

      case "empty-nook":
        useSystem.setState({ media: null, artwork: null });
        useHub.setState({ focus: EMPTY_FOCUS, tasks: [] });
        useIsland.setState({ hovered: true, peekDeck: "nook", dragOver: false, voiceOpen: false, workspaceOpen: false, clipsOpen: false, forceFocusBanner: false, focusBannerDismissed: true });
        break;

      case "empty-focus":
        useSystem.setState({ media: null, artwork: null });
        useHub.setState({ focus: EMPTY_FOCUS, tasks: [] });
        useIsland.setState({ hovered: true, peekDeck: "focus", dragOver: false, voiceOpen: false, workspaceOpen: false, clipsOpen: false, forceFocusBanner: false, focusBannerDismissed: true });
        break;

      case "growth": {
        useSystem.setState({ media: null, artwork: null });
        useHub.setState({ focus: EMPTY_FOCUS });
        const base = useMentor.getState().state!;
        const week = Array.from({ length: 7 }, (_, i) => {
          const d = new Date();
          d.setDate(d.getDate() - (6 - i));
          const xp = [80, 64, 20, 95, 55, 0, 35][i]!;
          return { date: localDateIso(d), xp, shortMin: [12, 8, 25, 4, 10, 30, 10][i]!, focusMin: xp / 2, growth: xp >= 50 };
        });
        useMentor.setState({ state: { ...base, week }, watching: null });
        useIsland.setState({ hovered: true, peekDeck: "growth", dragOver: false, voiceOpen: false, workspaceOpen: false, clipsOpen: false, forceFocusBanner: false, focusBannerDismissed: true });
        break;
      }

      case "nook-events":
        seedMedia(srcIndex, true);
        useHub.setState({ focus: EMPTY_FOCUS, tasks: buildSampleTasks().slice(0, 2) });
        useIsland.setState({ hovered: true, peekDeck: "nook", dragOver: false, voiceOpen: false, workspaceOpen: false, clipsOpen: false, forceFocusBanner: false, focusBannerDismissed: true });
        break;

      case "event-soon":
      case "event-now":
      case "compact-soon": {
        useSystem.setState({ media: null, artwork: null });
        useHub.setState({ focus: EMPTY_FOCUS });
        const events = buildSampleEvents(id === "event-now" ? "now" : "soon");
        const u = upcoming(events, new Date(), 10);
        if (u) remember(u);
        useAgenda.setState({ events, card: id === "compact-soon" ? null : u, soon: u && !u.started ? u : null });
        useIsland.setState({ hovered: false, dragOver: false, voiceOpen: false, workspaceOpen: false, clipsOpen: false, forceFocusBanner: false, focusBannerDismissed: false });
        break;
      }

      case "gain":
        useSystem.setState({ media: null, artwork: null });
        useHub.setState({ focus: EMPTY_FOCUS });
        useMentor.setState({ gain: { xp: 15, at: Date.now() } });
        useIsland.setState({ hovered: false, dragOver: false, voiceOpen: false, workspaceOpen: false, clipsOpen: false, forceFocusBanner: false, focusBannerDismissed: false });
        break;

      case "moment-day":
      case "moment-level":
        useSystem.setState({ media: null, artwork: null });
        useHub.setState({ focus: EMPTY_FOCUS });
        useMentor.setState({
          moment: id === "moment-day" ? { kind: "day", streak: 6, best: false, xp: 15, at: Date.now() } : { kind: "level", level: 4, name: "Собранный", xp: 55, at: Date.now() },
        });
        useIsland.setState({ hovered: false, dragOver: false, voiceOpen: false, workspaceOpen: false, clipsOpen: false, forceFocusBanner: false, focusBannerDismissed: false });
        break;

      case "growth-card":
        useSystem.setState({ media: null, artwork: null });
        useHub.setState({ focus: EMPTY_FOCUS });
        useIsland.setState({ hovered: true, peekDeck: "growth", growthCard: true, dragOver: false, voiceOpen: false, workspaceOpen: false, clipsOpen: false, forceFocusBanner: false, focusBannerDismissed: true });
        break;

      case "focus-cal":
        useSystem.setState({ media: null, artwork: null });
        useHub.setState({ focus: EMPTY_FOCUS });
        useIsland.setState({ hovered: true, peekDeck: "focus", dragOver: false, voiceOpen: false, workspaceOpen: false, clipsOpen: false, forceFocusBanner: false, focusBannerDismissed: true });
        break;

      case "watching":
        useSystem.setState({ media: null, artwork: null });
        useHub.setState({ focus: EMPTY_FOCUS });
        useMentor.setState({ watching: { site: "tiktok", label: "TikTok", usedSec: 452, limitSec: 600, strict: null } });
        useIsland.setState({ hovered: false, dragOver: false, voiceOpen: false, workspaceOpen: false, clipsOpen: false, forceFocusBanner: false, focusBannerDismissed: false });
        break;

      case "compact":
      case "skin-amber":
      case "skin-orb":
      case "skin-ice":
      case "skin-pixel":
      case "skin-minimal":
        if (id.startsWith("skin-")) {
          selectSkin(normalizeSkin(id.slice("skin-".length)));
        }
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
      case "tray-empty":
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
      case "drop-send":
        useIsland.setState({
          hovered: true,
          dragOver: true,
          dropOnSend: id === "drop-send",
          voiceOpen: false,
          workspaceOpen: false,
          clipsOpen: false,
          forceFocusBanner: false,
        });
        break;

      case "sound":
      case "sound-single":
      case "sound-low":
      case "sound-empty":
        seedMedia(srcIndex, true);
        useHub.setState({ focus: EMPTY_FOCUS });
        useIsland.setState({ hovered: true, peekDeck: "sound", dragOver: false, voiceOpen: false, workspaceOpen: false, clipsOpen: false, forceFocusBanner: false, focusBannerDismissed: true });
        break;

      case "hello":
        useSystem.setState({ media: null, artwork: null });
        useHub.setState({ focus: EMPTY_FOCUS });
        useIsland.setState({ hovered: false, dragOver: false, voiceOpen: false, workspaceOpen: false, clipsOpen: false, forceFocusBanner: false, focusBannerDismissed: false });
        useBluetooth.setState({ hello: { id: "qa-pods", reason: "connected", at: Date.now() } });
        break;

      case "pinned":
        seedMedia(srcIndex, true);
        useHub.setState({ focus: EMPTY_FOCUS });
        useIsland.setState({ hovered: false, peekDeck: "nook", dragOver: false, voiceOpen: false, workspaceOpen: false, clipsOpen: false, forceFocusBanner: false, focusBannerDismissed: true });
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

    const skinParam = params.get("skin");
    if (skinParam) selectSkin(normalizeSkin(skinParam));

    const actParam = params.get("activity") as Activity | null;
    if (actParam && ACTIVITIES.some((a) => a.id === actParam)) {
      if (!presetParam) applyPreset("compact", initialSrc);
      selectActivity(actParam);
    }

    const emoteParam = params.get("emote") as Emote | null;
    if (emoteParam) window.setTimeout(() => triggerEmote(emoteParam), 120);
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

        <div style={styles.skinRow}>
          <span style={styles.sourceHint}>Облик:</span>
          {SKINS.map((s) => (
            <button
              key={s.id}
              type="button"
              data-skin-pick={s.id}
              onClick={() => selectSkin(s.id)}
              style={{
                ...styles.sourceChip,
                ...(activeSkin === s.id ? styles.sourceChipActive : null),
              }}
            >
              {s.label}
            </button>
          ))}
        </div>

        <div style={styles.skinRow}>
          <span style={styles.sourceHint}>Работа:</span>
          <button
            type="button"
            onClick={() => selectActivity(null)}
            style={{
              ...styles.sourceChip,
              ...(activeActivity === null ? styles.sourceChipActive : null),
            }}
          >
            Нет
          </button>
          {ACTIVITIES.map((a) => (
            <button
              key={a.id}
              type="button"
              data-activity-pick={a.id}
              onClick={() => selectActivity(a.id)}
              style={{
                ...styles.sourceChip,
                ...(activeActivity === a.id ? styles.sourceChipActive : null),
              }}
            >
              {a.label}
            </button>
          ))}
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
  skinRow: {
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
