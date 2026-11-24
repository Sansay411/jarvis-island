// Pure helpers for Settings → Голос → microphone: the meter scale, number
// formatting and the guided test's prompts.

/** The meter spans -75…-5 dBFS so even a quiet room's floor stays visible. */
export const METER_MIN_DB = -75;
export const METER_MAX_DB = -5;

export type MicGainMode = "auto" | "manual";
export type NoiseMode = "off" | "auto" | "strong";
export type MicTestPhase = "idle" | "silence" | "speech" | "analyzing" | "done" | "error";

export interface MicMeter {
  db: number;
  floorDb: number;
  gainDb: number;
  speech: boolean;
  device: string;
  error: string | null;
}

export interface MicTestResult {
  deviceName: string;
  noiseFloorDb: number;
  postFloorDb: number;
  speechLevelDb: number;
  postSpeechDb: number;
  snrDb: number;
  appliedGainDb: number;
  recommendedGainDb: number;
  transcript: string;
  wakeDetected: boolean;
  sttAvailable: boolean;
  advice: string;
  severity: "ok" | "warn" | "error";
}

/** 0…1 position of a dBFS level on the meter. */
export function meterFraction(db: number): number {
  if (!Number.isFinite(db)) return 0;
  const f = (db - METER_MIN_DB) / (METER_MAX_DB - METER_MIN_DB);
  return Math.min(1, Math.max(0, f));
}

/** «−32 дБ» with a real minus sign; «—» for no signal. */
export function formatDb(db: number): string {
  if (!Number.isFinite(db) || db <= -89.5) return "—";
  const r = Math.round(db);
  return r < 0 ? `−${Math.abs(r)} дБ` : `${r} дБ`;
}

/** Gain with an explicit sign: «+18 дБ», «0 дБ», «−4 дБ». */
export function formatGain(db: number): string {
  if (!Number.isFinite(db)) return "0 дБ";
  const r = Math.round(db);
  if (r > 0) return `+${r} дБ`;
  if (r < 0) return `−${Math.abs(r)} дБ`;
  return "0 дБ";
}

/** The slider's range for manual sensitivity. */
export function clampGain(db: number): number {
  if (!Number.isFinite(db)) return 0;
  return Math.min(30, Math.max(-6, Math.round(db)));
}

/** What the user is asked to do in each phase of the test. */
export function phasePrompt(phase: MicTestPhase): string {
  switch (phase) {
    case "silence":
      return "Помолчите пару секунд: слушаю тишину в комнате";
    case "speech":
      return "Скажите: «Джарвис, какая сегодня погода?»";
    case "analyzing":
      return "Разбираю запись…";
    case "error":
      return "Проверка не удалась";
    default:
      return "";
  }
}

/** Overall progress 0…1 across the silence (2 s) and speech (4.5 s) phases. */
export function testProgress(phase: MicTestPhase, progress: number): number {
  const p = Math.min(1, Math.max(0, Number.isFinite(progress) ? progress : 0));
  const silence = 2 / 6.5;
  if (phase === "silence") return p * silence;
  if (phase === "speech") return silence + p * (1 - silence);
  if (phase === "analyzing" || phase === "done") return 1;
  return 0;
}

/** Signal over noise in plain words for the result card. */
export function snrWord(snrDb: number): string {
  if (snrDb >= 25) return "отлично";
  if (snrDb >= 15) return "хорошо";
  if (snrDb >= 10) return "терпимо";
  return "мало";
}
