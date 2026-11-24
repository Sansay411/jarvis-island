// Pure logic for the first-run «Знакомство» onboarding flow: trigger rules,
// memory-fact formatting, goal-to-mentor-track mapping, and applying answers
// to settings while respecting skipped steps.

import type { PersonaStyle, Settings } from "../settings/SettingsApp";

export const ONBOARDING_VERSION = 1;
export const ONBOARDING_MAX_DISMISSALS = 3;

export type OnboardingStep = 0 | 1 | 2 | 3 | 4 | 5 | 6;
export type OccupationKind = "work" | "study" | "projects" | "other";
export type GoalId = "focus" | "study" | "health" | "music" | "coding" | "planning";

export interface OnboardingSettings {
  done: boolean;
  version: number;
  dismissedCount: number;
}

export interface OnboardingAnswers {
  userName: string;
  address: "ty" | "vy";
  style: PersonaStyle;
  occupation: OccupationKind | null;
  occupationDetail: string;
  goals: GoalId[];
  workStart: string;
  workEnd: string;
  wakeTime: string;
  sleepTime: string;
  inputDevice: string;
  wakeWord: boolean;
}

export const DEFAULT_ONBOARDING: OnboardingSettings = {
  done: false,
  version: 0,
  dismissedCount: 0,
};

export const STYLE_OPTIONS: ReadonlyArray<{
  id: "warm" | "witty" | "concise";
  label: string;
  hint: string;
}> = [
  { id: "warm", label: "Тепло", hint: "Живой и дружелюбный тон" },
  { id: "witty", label: "С юмором", hint: "Лёгкая ирония и остроумие" },
  { id: "concise", label: "Кратко", hint: "Чётко и только по делу" },
];

export const OCCUPATION_OPTIONS: ReadonlyArray<{
  id: OccupationKind;
  label: string;
  placeholder: string;
}> = [
  { id: "work", label: "Работаю", placeholder: "Например: разработчик, дизайнер, менеджер…" },
  { id: "study", label: "Учусь", placeholder: "Например: изучаю Rust, 3 курс, готовлюсь к экзаменам…" },
  { id: "projects", label: "Свои проекты", placeholder: "Например: делаю приложение, веду канал, фриланс…" },
  { id: "other", label: "Другое", placeholder: "Пара слов о том, чем вы сейчас увлечены…" },
];

export const GOAL_OPTIONS: ReadonlyArray<{
  id: GoalId;
  label: string;
  tracks: readonly string[];
}> = [
  { id: "focus", label: "Фокус и продуктивность", tracks: ["mind", "projects"] },
  { id: "coding", label: "Программирование", tracks: ["code"] },
  { id: "study", label: "Учёба и языки", tracks: ["english", "mind"] },
  { id: "health", label: "Здоровье и режим", tracks: ["body"] },
  { id: "planning", label: "Планирование дня", tracks: ["projects"] },
  { id: "music", label: "Музыка и отдых", tracks: [] },
];

const CANONICAL_TRACKS = ["code", "english", "mind", "body", "projects"] as const;
const TIME_RE = /^([01]\d|2[0-3]):[0-5]\d$/;

/** Whether the first-run «Знакомство» flow should auto-open on island start. */
export function shouldAutoOpenOnboarding(
  settings: Pick<Settings, "persona"> & { onboarding?: Partial<OnboardingSettings> },
  memoriesCount: number,
): boolean {
  const ob = settings.onboarding;
  if (ob?.done) return false;
  if ((ob?.dismissedCount ?? 0) >= ONBOARDING_MAX_DISMISSALS) return false;
  if ((settings.persona?.userName ?? "").trim().length > 0) return false;
  if (memoriesCount > 0) return false;
  return true;
}

export function isValidTime(value: string): boolean {
  return TIME_RE.test(value.trim());
}

/** Map selected onboarding goals to canonical mentor tracks, or null if none map. */
export function mapGoalsToMentorTracks(goals: readonly GoalId[]): string[] | null {
  const picked = new Set<string>();
  for (const id of goals) {
    const opt = GOAL_OPTIONS.find((g) => g.id === id);
    if (!opt) continue;
    for (const track of opt.tracks) picked.add(track);
  }
  if (picked.size === 0) return null;
  return CANONICAL_TRACKS.filter((t) => picked.has(t));
}

function cleanText(raw: string): string {
  return raw.replace(/\s+/g, " ").trim().replace(/[.!?]+$/, "");
}

function formatStudyFact(detail: string): string {
  const lower = detail.toLowerCase();
  if (/^(изуча|учит|пишет|делает|готовит|проходит|осваива)/.test(lower)) {
    return `Пользователь — студент, ${detail}`;
  }
  return `Пользователь — студент, изучает ${detail}`;
}

/** Build short Russian memory facts from non-skipped onboarding answers. */
export function buildMemoryFacts(
  answers: Pick<OnboardingAnswers, "occupation" | "occupationDetail" | "goals" | "wakeTime" | "sleepTime">,
  skippedSteps: ReadonlySet<number>,
): string[] {
  const facts: string[] = [];

  if (!skippedSteps.has(2)) {
    const detail = cleanText(answers.occupationDetail);
    switch (answers.occupation) {
      case "study":
        facts.push(detail ? formatStudyFact(detail) : "Пользователь учится");
        break;
      case "work":
        facts.push(detail ? `Пользователь работает: ${detail}` : "Пользователь работает");
        break;
      case "projects":
        facts.push(
          detail ? `Пользователь занимается своими проектами: ${detail}` : "Пользователь занимается своими проектами",
        );
        break;
      case "other":
        if (detail) facts.push(`Чем занимается пользователь: ${detail}`);
        break;
      default:
        if (detail) facts.push(`Чем занимается пользователь: ${detail}`);
        break;
    }
  }

  if (!skippedSteps.has(3) && answers.goals.length > 0) {
    const labels = GOAL_OPTIONS.filter((g) => answers.goals.includes(g.id)).map((g) => g.label.toLowerCase());
    if (labels.length > 0) {
      facts.push(`Цели и интересы пользователя: ${labels.join(", ")}`);
    }
  }

  if (!skippedSteps.has(4)) {
    const wake = isValidTime(answers.wakeTime) ? answers.wakeTime.trim() : "";
    const sleep = isValidTime(answers.sleepTime) ? answers.sleepTime.trim() : "";
    if (wake && sleep) {
      facts.push(`Обычный режим дня: подъём около ${wake}, сон около ${sleep}`);
    } else if (wake) {
      facts.push(`Обычно просыпается около ${wake}`);
    } else if (sleep) {
      facts.push(`Обычно ложится спать около ${sleep}`);
    }
  }

  return facts;
}

/** Apply non-skipped onboarding answers to a copy of the user's settings. */
export function applyOnboardingToSettings(
  base: Settings,
  answers: OnboardingAnswers,
  skippedSteps: ReadonlySet<number>,
): Settings {
  const next: Settings = structuredClone(base);

  if (!skippedSteps.has(1)) {
    next.persona.userName = answers.userName.trim();
    next.persona.address = answers.address;
    next.persona.style = answers.style;
  }

  if (!skippedSteps.has(3)) {
    const tracks = mapGoalsToMentorTracks(answers.goals);
    if (tracks) {
      next.mentor.tracks = tracks;
    }
  }

  if (!skippedSteps.has(4)) {
    if (isValidTime(answers.workStart)) next.planner.workStart = answers.workStart.trim();
    if (isValidTime(answers.workEnd)) next.planner.workEnd = answers.workEnd.trim();
  }

  if (!skippedSteps.has(5)) {
    next.voice.inputDevice = answers.inputDevice;
    next.voice.wakeWord = answers.wakeWord;
  }

  next.onboarding = {
    done: true,
    version: ONBOARDING_VERSION,
    dismissedCount: base.onboarding?.dismissedCount ?? 0,
  };

  return next;
}

export function answersFromSettings(settings: Settings): OnboardingAnswers {
  return {
    userName: settings.persona.userName ?? "",
    address: settings.persona.address === "vy" ? "vy" : "ty",
    // Sarcasm and «балагур» are chosen in Settings; the first-run cards keep them.
    style: (["witty", "sarcastic", "playful", "warm", "concise"] as const).find((st) => st === settings.persona.style) ?? "witty",
    occupation: null,
    occupationDetail: "",
    goals: [],
    workStart: isValidTime(settings.planner.workStart) ? settings.planner.workStart : "09:00",
    workEnd: isValidTime(settings.planner.workEnd) ? settings.planner.workEnd : "18:00",
    wakeTime: "08:00",
    sleepTime: "23:30",
    inputDevice: settings.voice.inputDevice ?? "",
    wakeWord: Boolean(settings.voice.wakeWord),
  };
}
