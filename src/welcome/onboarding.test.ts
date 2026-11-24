import { describe, expect, it } from "vitest";
import type { Settings } from "../settings/SettingsApp";
import {
  ONBOARDING_VERSION,
  answersFromSettings,
  applyOnboardingToSettings,
  buildMemoryFacts,
  mapGoalsToMentorTracks,
  shouldAutoOpenOnboarding,
  type OnboardingAnswers,
} from "./onboarding";

function makeSettings(overrides: Partial<Settings> = {}): Settings {
  return {
    provider: { preset: "jarvis-cloud", baseUrl: "", model: "deepseek-flash", visionModel: "", temperature: 0.3 },
    voice: {
      enabled: true,
      engine: "local",
      sttModel: "gigaam-v3",
      ttsVoice: "",
      ttsRate: 1.1,
      ttsPitch: 0,
      autoSpeakReplies: false,
      wakeWord: false,
      realtimeUrl: "",
      model: "",
      voice: "",
      reuseLlmKey: true,
      inputDevice: "",
      outputDevice: "",
      duckMedia: true,
      cloudStt: true,
    },
    persona: { userName: "", address: "ty", style: "witty" },
    shortcuts: { workspace: "Ctrl+Alt+J", voice: "Ctrl+Alt+K", clipboard: "Ctrl+Alt+V" },
    privacy: { storeHistory: true, allowScreen: true, allowClipboardRead: false },
    clipboard: { enabled: true, maxItems: 100, keepDays: 7, skipSecrets: true, ignoredApps: [] },
    agent: { maxSteps: 24, confirmModerate: true, fullControl: false, projectRoots: [], learnSkills: true },
    launchAtStartup: false,
    reduceMotion: false,
    updates: { autoCheck: true },
    planner: { workStart: "09:00", workEnd: "18:00", dayStartHour: 8, googleClientId: "", googleCalendarId: "primary", islandRemind: 10 },
    mentor: {
      guard: true,
      shortMinutes: 10,
      youtubeMinutes: 30,
      earn: true,
      strictFocus: true,
      speak: true,
      tracks: ["code", "english", "mind", "body", "projects"],
      learningWords: [],
      nightFrom: "",
      nightTo: "",
      ifThen: "",
    },
    telegram: { enabled: true, chatId: 0, botUsername: "", morning: "08:30", evening: "21:30", remindBefore: 10, agent: true },
    onboarding: { done: false, version: 0, dismissedCount: 0 },
    ...overrides,
  };
}

describe("shouldAutoOpenOnboarding", () => {
  it("opens for a brand-new user with no name or memories", () => {
    expect(shouldAutoOpenOnboarding(makeSettings(), 0)).toBe(true);
  });

  it("skips when already completed", () => {
    const s = makeSettings({ onboarding: { done: true, version: 1, dismissedCount: 0 } });
    expect(shouldAutoOpenOnboarding(s, 0)).toBe(false);
  });

  it("asks again up to 3 dismissals, then stops", () => {
    expect(shouldAutoOpenOnboarding(makeSettings({ onboarding: { done: false, version: 0, dismissedCount: 1 } }), 0)).toBe(true);
    expect(shouldAutoOpenOnboarding(makeSettings({ onboarding: { done: false, version: 0, dismissedCount: 2 } }), 0)).toBe(true);
    expect(shouldAutoOpenOnboarding(makeSettings({ onboarding: { done: false, version: 0, dismissedCount: 3 } }), 0)).toBe(false);
    expect(shouldAutoOpenOnboarding(makeSettings({ onboarding: { done: false, version: 0, dismissedCount: 5 } }), 0)).toBe(false);
  });

  it("treats existing persona name or memories as already onboarded", () => {
    expect(shouldAutoOpenOnboarding(makeSettings({ persona: { userName: "Мария", address: "vy", style: "warm" } }), 0)).toBe(false);
    expect(shouldAutoOpenOnboarding(makeSettings({ persona: { userName: "   ", address: "ty", style: "witty" } }), 0)).toBe(true);
    expect(shouldAutoOpenOnboarding(makeSettings(), 2)).toBe(false);
  });
});

describe("buildMemoryFacts", () => {
  const baseAnswers: OnboardingAnswers = {
    userName: "Алексей",
    address: "ty",
    style: "warm",
    occupation: "study",
    occupationDetail: "Rust",
    goals: ["coding", "focus"],
    workStart: "10:00",
    workEnd: "19:00",
    wakeTime: "08:00",
    sleepTime: "23:30",
    inputDevice: "USB Mic",
    wakeWord: true,
  };

  it("creates concise Russian facts for answered steps", () => {
    const facts = buildMemoryFacts(baseAnswers, new Set());
    expect(facts).toEqual([
      "Пользователь — студент, изучает Rust",
      "Цели и интересы пользователя: фокус и продуктивность, программирование",
      "Обычный режим дня: подъём около 08:00, сон около 23:30",
    ]);
  });

  it("handles verb-led study details and work/projects/other occupations", () => {
    expect(
      buildMemoryFacts({ ...baseAnswers, occupation: "study", occupationDetail: "изучает системное программирование." }, new Set([3, 4])),
    ).toEqual(["Пользователь — студент, изучает системное программирование"]);

    expect(
      buildMemoryFacts({ ...baseAnswers, occupation: "work", occupationDetail: "дизайнер интерфейсов" }, new Set([3, 4])),
    ).toEqual(["Пользователь работает: дизайнер интерфейсов"]);

    expect(
      buildMemoryFacts({ ...baseAnswers, occupation: "projects", occupationDetail: "" }, new Set([3, 4])),
    ).toEqual(["Пользователь занимается своими проектами"]);

    expect(
      buildMemoryFacts({ ...baseAnswers, occupation: "other", occupationDetail: "" }, new Set([3, 4])),
    ).toEqual([]);
  });

  it("saves nothing for skipped steps or empty rhythm fields", () => {
    expect(buildMemoryFacts(baseAnswers, new Set([2, 3, 4]))).toEqual([]);
    expect(buildMemoryFacts({ ...baseAnswers, wakeTime: "07:30", sleepTime: "" }, new Set([2, 3]))).toEqual([
      "Обычно просыпается около 07:30",
    ]);
    expect(buildMemoryFacts({ ...baseAnswers, wakeTime: "", sleepTime: "00:15" }, new Set([2, 3]))).toEqual([
      "Обычно ложится спать около 00:15",
    ]);
    expect(buildMemoryFacts({ ...baseAnswers, wakeTime: "bad", sleepTime: "" }, new Set([2, 3]))).toEqual([]);
  });
});

describe("mapGoalsToMentorTracks", () => {
  it("maps goals cleanly in canonical order without duplicates", () => {
    expect(mapGoalsToMentorTracks(["planning", "coding", "focus"])).toEqual(["code", "mind", "projects"]);
    expect(mapGoalsToMentorTracks(["health", "study"])).toEqual(["english", "mind", "body"]);
  });

  it("returns null when selected goals do not map to mentor tracks", () => {
    expect(mapGoalsToMentorTracks(["music"])).toBeNull();
    expect(mapGoalsToMentorTracks([])).toBeNull();
  });
});

describe("applyOnboardingToSettings", () => {
  it("writes only non-skipped sections and marks onboarding done", () => {
    const base = makeSettings();
    const answers: OnboardingAnswers = {
      ...answersFromSettings(base),
      userName: "  Анна ",
      address: "vy",
      style: "warm",
      goals: ["coding", "health"],
      workStart: "10:00",
      workEnd: "19:30",
      inputDevice: "Shure MV7",
      wakeWord: true,
    };

    const updated = applyOnboardingToSettings(base, answers, new Set([4]));
    expect(updated.persona).toEqual({ userName: "Анна", address: "vy", style: "warm" });
    expect(updated.mentor.tracks).toEqual(["code", "body"]);
    // Step 4 was skipped, so work hours remain untouched.
    expect(updated.planner.workStart).toBe("09:00");
    expect(updated.planner.workEnd).toBe("18:00");
    expect(updated.voice.inputDevice).toBe("Shure MV7");
    expect(updated.voice.wakeWord).toBe(true);
    expect(updated.onboarding).toEqual({ done: true, version: ONBOARDING_VERSION, dismissedCount: 0 });
  });

  it("preserves mentor tracks when only non-mapped goals are chosen", () => {
    const base = makeSettings();
    const answers: OnboardingAnswers = { ...answersFromSettings(base), goals: ["music"] };
    const updated = applyOnboardingToSettings(base, answers, new Set([1, 4, 5]));
    expect(updated.mentor.tracks).toEqual(base.mentor.tracks);
    expect(updated.persona.userName).toBe("");
    expect(updated.onboarding.done).toBe(true);
  });
});
