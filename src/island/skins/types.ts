// Skin definitions, palette tokens and shared face types.

import type { CSSProperties } from "react";
import type { MotionValue } from "motion/react";
import type { Activity, Emote } from "../../store/emote";

export type EyeMood = "idle" | "attentive" | "thinking" | "listening" | "speaking" | "happy" | "error" | "sleep";

export type SkinId = "amber" | "orb" | "ice" | "pixel" | "minimal";

export const DEFAULT_SKIN: SkinId = "amber";

export interface SkinMeta {
  id: SkinId;
  label: string;
  desc: string;
  accent: string;
  accentHi: string;
  accentLo: string;
  glow: string;
  rgb: string;
}

export const SKINS: SkinMeta[] = [
  {
    id: "amber",
    label: "Янтарь",
    desc: "Классические тёплые линзы",
    accent: "#ffb23f",
    accentHi: "#ffd88f",
    accentLo: "#e2861c",
    glow: "rgba(255, 178, 63, 0.5)",
    rgb: "255, 178, 63",
  },
  {
    id: "orb",
    label: "Сфера",
    desc: "Облако светящихся точек",
    accent: "#f3e4c6",
    accentHi: "#fffdf6",
    accentLo: "#c8a978",
    glow: "rgba(243, 228, 198, 0.48)",
    rgb: "243, 228, 198",
  },
  {
    id: "ice",
    label: "Лёд",
    desc: "Холодное стекло и спокойный взгляд",
    accent: "#7ce8ff",
    accentHi: "#e8fbff",
    accentLo: "#29b4d8",
    glow: "rgba(124, 232, 255, 0.48)",
    rgb: "124, 232, 255",
  },
  {
    id: "pixel",
    label: "Пиксель",
    desc: "Чёткий ретро-экран и шаговая мимика",
    accent: "#6ff0a6",
    accentHi: "#c8ffe0",
    accentLo: "#2bb56e",
    glow: "rgba(111, 240, 166, 0.44)",
    rgb: "111, 240, 166",
  },
  {
    id: "minimal",
    label: "Минимал",
    desc: "Два тонких штриха без лишнего",
    accent: "#ecebe6",
    accentHi: "#ffffff",
    accentLo: "#a8a8b0",
    glow: "rgba(236, 235, 230, 0.34)",
    rgb: "236, 235, 230",
  },
];

const SKIN_MAP = new Map<string, SkinMeta>(SKINS.map((s) => [s.id, s]));

export function normalizeSkin(raw?: string | null): SkinId {
  if (raw && SKIN_MAP.has(raw)) return raw as SkinId;
  return DEFAULT_SKIN;
}

export function skinMeta(skin: SkinId): SkinMeta {
  return SKIN_MAP.get(skin) ?? SKINS[0]!;
}

/** CSS custom properties exposing the active skin's presence accent. */
export function skinCssVars(skin: SkinId): CSSProperties {
  const m = skinMeta(skin);
  return {
    ["--skin-accent" as string]: m.accent,
    ["--skin-accent-hi" as string]: m.accentHi,
    ["--skin-accent-lo" as string]: m.accentLo,
    ["--skin-glow" as string]: m.glow,
    ["--skin-rgb" as string]: m.rgb,
  };
}

export interface EyesProps {
  mood: EyeMood;
  /** Visual look override; omit to follow the saved setting in `usePrefs`. */
  skin?: SkinId;
  /** 0..1 live audio level (listening/speaking). */
  level?: MotionValue<number>;
  /** -1..1 horizontal gaze. */
  lookX?: MotionValue<number>;
  /** Vertical gaze offset in px (e.g. looking down at a drop zone). */
  lookY?: number;
  scale?: number;
  /** A one-shot expression; `emoteSeq` replays the same one. */
  emote?: Emote | null;
  emoteSeq?: number;
  /** Music is playing: nod along. */
  bob?: boolean;
  /** Late night: heavy lids, slow blinks. */
  sleepy?: boolean;
  /** A timer's last seconds: look at it (to the right), blink often. */
  watch?: boolean;
  /** A timer or reminder is ringing: jump. */
  alarm?: boolean;
  /** What JARVIS is busy with while a tool runs. */
  activity?: Activity | null;
  /** Double click on the eyes. */
  onBoop?: () => void;
}
