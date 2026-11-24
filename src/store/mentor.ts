// The mentor's state (XP, level, streak, short-video minutes, what is on
// screen now) and the calls «Перезагрузка» makes. Outside Tauri (visual QA)
// it answers with sample data so every screen can be inspected.

import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { isTauri } from "../lib/ipc";

export interface Budget {
  usedSec: number;
  limitSec: number;
}

export interface Watching {
  site: string;
  label: string;
  usedSec: number;
  limitSec: number;
  strict: string | null;
}

export interface DayStat {
  date: string;
  xp: number;
  shortMin: number;
  focusMin: number;
  growth: boolean;
}

export interface MentorState {
  guard: boolean;
  xpTotal: number;
  level: number;
  levelName: string;
  levelFrom: number;
  levelTo: number;
  todayXp: number;
  goalXp: number;
  streak: number;
  bestStreak: number;
  short: Budget;
  youtube: Budget;
  earnedSec: number;
  strict: string | null;
  watching: Watching | null;
  closesToday: number;
  focusMinToday: number;
  week: DayStat[];
  ifThen: string;
  tracks: string[];
}

export interface Intervention {
  id: string;
  reason: "budget" | "strict" | "voluntary";
  site: string;
  label: string;
  usedSec: number;
  strict: string | null;
  why: string;
  title: string;
  url: string | null;
  closed: boolean;
  ifThen: string;
  at: string;
  /** Open straight on one activity (the island's quick actions). */
  screen?: "feed" | "games" | "workout" | "breath" | "step";
}

export type ResetScreen = NonNullable<Intervention["screen"]>;

/** A step worth a moment on the island: a new level or a counted day. */
export type Moment =
  | { kind: "level"; level: number; name: string; xp: number; at: number }
  | { kind: "day"; streak: number; best: boolean; xp: number; at: number };

/** What changed between two states: a moment worth a card, and the XP
 *  gained (a short «+15 XP» in the compact island). */
export function momentOf(prev: MentorState | null, next: MentorState, at = Date.now()): { moment: Moment | null; gain: number } {
  if (!prev) return { moment: null, gain: 0 };
  const gain = Math.max(0, next.xpTotal - prev.xpTotal);
  if (next.level > prev.level) return { moment: { kind: "level", level: next.level, name: next.levelName, xp: gain, at }, gain };
  const sameDay = prev.week.at(-1)?.date === next.week.at(-1)?.date;
  if (sameDay && prev.todayXp < prev.goalXp && next.todayXp >= next.goalXp) {
    return { moment: { kind: "day", streak: next.streak, best: next.streak > prev.bestStreak, xp: gain, at }, gain };
  }
  return { moment: null, gain };
}

export interface FeedCard {
  id: string;
  track: "code" | "english" | "body" | string;
  kind: "quiz" | "code" | "word" | "fact" | "challenge" | string;
  rare: boolean;
  review: boolean;
  data: Record<string, unknown>;
}

export interface Reward {
  xp: number;
  earnedSec: number;
  level: number;
  levelName: string;
  levelUp: boolean;
  streak: number;
  best: boolean;
  record: number;
}

export interface Step {
  kind: "event" | "task" | "repo" | string;
  title: string;
  subtitle: string;
  path: string | null;
  minutes: number;
}

export interface WorkoutMove {
  title: string;
  reps?: number;
  seconds?: number;
  hint: string;
}

const SAMPLE_STATE: MentorState = {
  guard: true,
  xpTotal: 420,
  level: 3,
  levelName: "Практик",
  levelFrom: 300,
  levelTo: 600,
  todayXp: 35,
  goalXp: 50,
  streak: 5,
  bestStreak: 9,
  short: { usedSec: 612, limitSec: 600 },
  youtube: { usedSec: 840, limitSec: 1800 },
  earnedSec: 0,
  strict: null,
  watching: null,
  closesToday: 1,
  focusMinToday: 25,
  week: [],
  ifThen: "",
  tracks: ["code", "english", "body", "projects"],
};

interface MentorStore {
  state: MentorState | null;
  pending: Intervention | null;
  watching: Watching | null;
  /** A new level or a counted day, shown on the island for a few seconds. */
  moment: Moment | null;
  /** XP just earned, a short chip in the compact island. */
  gain: { xp: number; at: number } | null;
  refresh: () => Promise<void>;
  showMoment: (moment: Moment) => void;
  dismissMoment: () => void;
  showGain: (xp: number) => void;
}

export const useMentor = create<MentorStore>((set, get) => ({
  state: isTauri() ? null : SAMPLE_STATE,
  pending: null,
  watching: null,
  moment: null,
  gain: null,
  showMoment: (moment) => {
    set({ moment });
    window.setTimeout(() => get().moment === moment && set({ moment: null }), 5200);
  },
  dismissMoment: () => set({ moment: null }),
  showGain: (xp) => {
    const gain = { xp, at: Date.now() };
    set({ gain });
    window.setTimeout(() => get().gain === gain && set({ gain: null }), 3600);
  },
  refresh: async () => {
    if (!isTauri()) return;
    try {
      set({ state: await invoke<MentorState>("mentor_state") });
    } catch {
      /* the island keeps the last state */
    }
  },
}));

let fed = false;
export async function startMentorFeed() {
  if (fed || !isTauri()) return;
  fed = true;
  await useMentor.getState().refresh();
  await listen<MentorState>("mentor:state", (e) => useMentor.setState({ state: e.payload, watching: e.payload.watching }));
  await listen<Watching | null>("mentor:watching", (e) => useMentor.setState({ watching: e.payload }));
}

/** The island celebrates growth: every state that brings XP is compared
 *  with the one before. Only the island window calls this. */
export function watchMoments(onMoment?: (m: Moment) => void) {
  return useMentor.subscribe((s, prev) => {
    if (!s.state || s.state === prev.state) return;
    const { moment, gain } = momentOf(prev.state, s.state);
    if (moment) {
      s.showMoment(moment);
      onMoment?.(moment);
    } else if (gain > 0) s.showGain(gain);
  });
}

export const mentorIpc = {
  pending: () => (isTauri() ? invoke<Intervention | null>("mentor_pending") : Promise.resolve(null)),
  openReset: (screen?: ResetScreen) => (isTauri() ? invoke<void>("mentor_open_reset", { screen: screen ?? null }) : Promise.resolve()),
  feed: (count = 7) => (isTauri() ? invoke<FeedCard[]>("mentor_feed", { count }) : Promise.resolve(SAMPLE_FEED)),
  answer: (id: string, correct: boolean, quick: boolean, xp: number) =>
    isTauri() ? invoke<void>("mentor_answer", { id, correct, quick, xp }) : Promise.resolve(),
  complete: (kind: string, track: string, title: string, minutes: number, score: number, xp: number, afterClose: boolean) =>
    isTauri()
      ? invoke<Reward>("mentor_complete", { kind, track, title, minutes, score, xp, afterClose })
      : Promise.resolve<Reward>({ xp: xp + (afterClose ? 15 : 0), earnedSec: kind === "game" ? 120 : 180, level: 3, levelName: "Практик", levelUp: false, streak: 6, best: score > 10, record: Math.max(score, 14) }),
  words: () =>
    isTauri()
      ? invoke<Array<[string, string]>>("mentor_words")
      : Promise.resolve<Array<[string, string]>>([
          ["eventually", "в конце концов"], ["actually", "на самом деле"], ["reliable", "надёжный"], ["to put off", "откладывать"],
          ["to figure out", "разобраться"], ["outcome", "результат"], ["thorough", "тщательный"], ["feasible", "осуществимый"],
          ["bottleneck", "узкое место"], ["trade-off", "компромисс"], ["meanwhile", "тем временем"], ["to give up", "сдаваться"],
        ]),
  /** Fresh cards from the model now; how many were added. */
  generate: (track?: string) => (isTauri() ? invoke<number>("mentor_generate", { track: track ?? null }) : Promise.resolve(0)),
  best: (game: string) => (isTauri() ? invoke<number>("mentor_best", { game }) : Promise.resolve(14)),
  workout: () =>
    isTauri()
      ? invoke<WorkoutMove[]>("mentor_workout")
      : Promise.resolve<WorkoutMove[]>([
          { title: "Приседания", reps: 20, hint: "Медленно вниз, быстро вверх" },
          { title: "Отжимания", reps: 10, hint: "Можно от стола" },
          { title: "Планка", seconds: 30, hint: "Тело — прямая линия" },
        ]),
  steps: () =>
    isTauri()
      ? invoke<Step[]>("mentor_steps")
      : Promise.resolve<Step[]>([
          { kind: "event", title: "Глубокая работа: jarvis-island", subtitle: "В календаре на 15:00", path: null, minutes: 15 },
          { kind: "task", title: "Подготовить еженедельный отчёт", subtitle: "Задача", path: null, minutes: 10 },
          { kind: "repo", title: "jarvis-island", subtitle: "Последний коммит: «Planner: Google-style week grid»", path: "E:\\Jarvis", minutes: 10 },
        ]),
  startStep: (title: string, minutes: number, path: string | null) =>
    isTauri() ? invoke<void>("mentor_start_step", { title, minutes, path }) : Promise.resolve(),
  allow: (title: string, url: string | null) => (isTauri() ? invoke<void>("mentor_allow", { title, url }) : Promise.resolve()),
  review: (wentWell: string, learned: string, tomorrow: string) =>
    isTauri() ? invoke<Reward>("mentor_review", { wentWell, learned, tomorrow }) : Promise.resolve<Reward>({ xp: 30, earnedSec: 0, level: 3, levelName: "Практик", levelUp: false, streak: 6, best: false, record: 0 }),
};

const SAMPLE_FEED: FeedCard[] = [
  { id: "s1", track: "code", kind: "code", rare: false, review: false, data: { title: "В каком порядке напечатается?", code: "setTimeout(() => console.log('A'), 0)\nPromise.resolve().then(() => console.log('B'))\nconsole.log('C')", options: ["A B C", "C B A", "C A B", "B C A"], answer: 1, explain: "Сначала синхронный код (C), потом микрозадачи — промисы (B), потом таймеры (A)." } },
  { id: "s2", track: "english", kind: "word", rare: true, review: false, data: { word: "eventually", ipa: "ɪˈventʃuəli", tr: "в конце концов", ex: "It will eventually work.", exRu: "В конце концов это заработает. (Не «эвентуально»!)", options: ["актуально", "в конце концов", "возможно", "срочно"], answer: 1 } },
  { id: "s3", track: "body", kind: "challenge", rare: false, review: false, data: { title: "20 приседаний", body: "Медленно вниз, быстро вверх, колени смотрят туда же, куда носки.", reps: 20 } },
  { id: "s4", track: "code", kind: "quiz", rare: false, review: true, data: { q: "Какой HTTP-статус вернуть, если пользователь не вошёл в систему?", options: ["401", "403", "404", "500"], answer: 0, explain: "401 — «кто ты?» (нет входа). 403 — «знаю, кто ты, но нельзя»." } },
  { id: "s5", track: "body", kind: "fact", rare: false, review: false, data: { title: "Прогулка против тяги", body: "Быстрая 15-минутная ходьба снижала тягу к шоколаду и сигаретам в контролируемых исследованиях. Тяга к ленте устроена так же — её снимает движение." } },
];

/** «5 минут», «1 минута», «3 минуты». */
export function minutesRu(n: number): string {
  const a = n % 10;
  const b = n % 100;
  const w = a === 1 && b !== 11 ? "минута" : a >= 2 && a <= 4 && (b < 12 || b > 14) ? "минуты" : "минут";
  return `${n} ${w}`;
}

/** «3 дня», «1 день», «5 дней». */
export function daysRu(n: number): string {
  const a = n % 10;
  const b = n % 100;
  const w = a === 1 && b !== 11 ? "день" : a >= 2 && a <= 4 && (b < 12 || b > 14) ? "дня" : "дней";
  return `${n} ${w}`;
}
