import { describe, expect, it } from "vitest";
import {
  buildProbeBadges,
  formatContextBadge,
  formatTokensK,
  isLocalOrCustomPreset,
  type ModelProbe,
} from "./modelProbe";

describe("modelProbe helpers", () => {
  it("recognizes local and custom presets", () => {
    expect(isLocalOrCustomPreset("ollama")).toBe(true);
    expect(isLocalOrCustomPreset("lmstudio")).toBe(true);
    expect(isLocalOrCustomPreset("custom")).toBe(true);
    expect(isLocalOrCustomPreset("deepseek")).toBe(false);
    expect(isLocalOrCustomPreset("jarvis-cloud")).toBe(false);
  });

  it("formats token counts cleanly in k", () => {
    expect(formatTokensK(4096)).toBe("4k");
    expect(formatTokensK(8192)).toBe("8k");
    expect(formatTokensK(32768)).toBe("32k");
    expect(formatTokensK(14120)).toBe("14k");
    expect(formatTokensK(512)).toBe("512");
  });

  it("formats context badge for ok, warn, and bad states", () => {
    expect(formatContextBadge({ modelMax: 32768, loaded: 8192, needed: 14000 })).toEqual({
      label: "Контекст 8k из нужных ~14k",
      status: "bad",
    });
    expect(formatContextBadge({ modelMax: 32768, loaded: 32768, needed: 14000 })).toEqual({
      label: "Контекст 32k из нужных ~14k",
      status: "ok",
    });
    expect(formatContextBadge({ modelMax: 32768, loaded: null, needed: 14000 })).toEqual({
      label: "Контекст до 32k (нужно ~14k)",
      status: "warn",
    });
    expect(formatContextBadge({ modelMax: null, loaded: null, needed: 14000 })).toEqual({
      label: "Контекст неизвестен (нужно ~14k)",
      status: "warn",
    });
  });

  it("builds three badges with glyphs and reflects visionModel fallback", () => {
    const probe: ModelProbe = {
      vision: "no",
      tools: "yes",
      context: { modelMax: 32768, loaded: 32768, needed: 14000 },
      verdict: "warn",
      summary: "Работает без экрана",
      notes: ["Укажите модель для экрана"],
      serverKind: "ollama",
    };

    const withoutFallback = buildProbeBadges(probe, "");
    expect(withoutFallback.map((b) => [b.label, b.glyph, b.status])).toEqual([
      ["Видит экран", "✕", "bad"],
      ["Вызывает инструменты", "✓", "ok"],
      ["Контекст 32k из нужных ~14k", "✓", "ok"],
    ]);

    const withFallback = buildProbeBadges(probe, "qwen2.5vl:7b");
    expect(withFallback[0]).toMatchObject({
      label: "Видит экран",
      glyph: "✓",
      status: "ok",
      sub: "через qwen2.5vl:7b",
    });
  });
});
