import { describe, expect, it } from "vitest";
import { chatCost, issueToken, overLimit, secondsToMidnight, sttText, ttsUnits, usageFromSse, verifyToken } from "../src/core";

const limits = { chat: 300, tts: 300, sttSeconds: 1200, registrations: 5, usd: 3, tenge: 1500 };
const prices = { inMiss: 0.3, inHit: 0.006, out: 1.2, tool: 0.005, tts: 1, stt15: 1.5 };

describe("device tokens", () => {
  it("verifies its own signature and nothing else", async () => {
    const id = "123e4567-e89b-42d3-a456-426614174000";
    const t = await issueToken("secret", id);
    expect(await verifyToken("secret", t)).toBe(id);
    expect(await verifyToken("other", t)).toBeNull();
    expect(await verifyToken("secret", t.replace(id, "123e4567-e89b-42d3-a456-426614174001"))).toBeNull();
    expect(await verifyToken("secret", "sk-something")).toBeNull();
  });
});

describe("limits", () => {
  it("stops a device at its own limit and everyone at the budget", () => {
    expect(overLimit("chat", { chat: 299 }, {}, limits)).toBeNull();
    expect(overLimit("chat", { chat: 300 }, {}, limits)).toMatch(/устройства/);
    expect(overLimit("chat", {}, { usd: 3 }, limits)).toMatch(/бюджет/);
    expect(overLimit("tts", {}, { usd: 3 }, limits)).toBeNull();
    expect(overLimit("stt", { stt: 1200 }, {}, limits)).toMatch(/распознавания/);
    expect(overLimit("stt", {}, { tenge: 1500 }, limits)).toMatch(/бюджет/);
  });

  it("resets at UTC midnight", () => {
    expect(secondsToMidnight(new Date("2026-11-23T23:59:00Z"))).toBe(60);
  });
});

describe("costs", () => {
  it("prices cache hits apart from misses", () => {
    const c = chatCost({ prompt_tokens: 1_000_000, prompt_cache_hit_tokens: 500_000, completion_tokens: 1_000_000 }, prices);
    expect(c).toBeCloseTo(0.15 + 0.003 + 1.2);
    expect(ttsUnits("а".repeat(251))).toBe(2);
  });

  it("finds the usage chunk at the end of a stream", () => {
    const sse = 'data: {"choices":[{"delta":{"content":"hi"}}]}\n\ndata: {"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":2}}\n\ndata: [DONE]\n';
    expect(usageFromSse(sse)).toEqual({ prompt_tokens: 10, completion_tokens: 2 });
    expect(usageFromSse("data: [DONE]\n")).toBeNull();
  });
});

describe("SpeechKit answers", () => {
  it("prefers the normalised refinement and joins utterances", () => {
    const body = [
      { result: { final: { alternatives: [{ text: "открой браузер" }] } } },
      { result: { finalRefinement: { normalizedText: { alternatives: [{ text: "Открой браузер." }] } } } },
      { result: { final: { alternatives: [{ text: "громкость десять" }] } } },
      { result: { finalRefinement: { normalizedText: { alternatives: [{ text: "Громкость 10." }] } } } },
    ].map((m) => JSON.stringify(m)).join("\n");
    expect(sttText(body)).toBe("Открой браузер. Громкость 10.");
    expect(sttText(JSON.stringify({ result: { final: { alternatives: [{ text: "да" }] } } }))).toBe("да");
    expect(sttText("")).toBe("");
  });
});
