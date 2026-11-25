import { readFileSync } from "node:fs";
import { DatabaseSync, type SQLInputValue } from "node:sqlite";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { issueToken } from "../src/core";
import worker from "../src/index";

const secret = "test-secret";
const first = "123e4567-e89b-42d3-a456-426614174000";
const second = "123e4567-e89b-42d3-a456-426614174001";
const schema = readFileSync(new URL("../migrations/0001_usage.sql", import.meta.url), "utf8");

function localDb(sqlite: DatabaseSync) {
  return {
    prepare(sql: string) {
      const stmt = sqlite.prepare(sql);
      return {
        bind(...params: SQLInputValue[]) {
          const run = () => ({ meta: { changes: Number(stmt.run(...params).changes) }, results: [] });
          const all = () => ({ results: stmt.all(...params) });
          return { run, all, execute: /^SELECT/i.test(sql) ? all : run };
        },
      };
    },
    async batch(stmts: { execute(): unknown }[]) {
      sqlite.exec("BEGIN");
      try {
        const results = stmts.map((stmt) => stmt.execute());
        sqlite.exec("COMMIT");
        return results;
      } catch (e) {
        sqlite.exec("ROLLBACK");
        throw e;
      }
    },
  };
}

describe("cloud quota", () => {
  let sqlite: DatabaseSync;
  let db: ReturnType<typeof localDb>;
  let background: Promise<unknown>[];

  beforeEach(() => {
    sqlite = new DatabaseSync(":memory:");
    sqlite.exec(schema);
    db = localDb(sqlite);
    background = [];
  });

  afterEach(() => {
    sqlite.close();
    vi.unstubAllGlobals();
  });

  async function call(path: string, id: string, body: BodyInit, caps: Record<string, string> = {}) {
    const init: RequestInit & { duplex: "half" } = {
      method: "POST",
      headers: { authorization: `Bearer ${await issueToken(secret, id)}`, "cf-connecting-ip": "192.0.2.1" },
      body,
      duplex: "half",
    };
    const req = new Request(`https://jarvis.example/v1/${path}`, init);
    return worker.fetch(req, { DB: db, TOKEN_SECRET: secret, ...caps } as unknown as Parameters<typeof worker.fetch>[1], {
      waitUntil: (p: Promise<unknown>) => background.push(p),
    } as Parameters<typeof worker.fetch>[2]);
  }

  function used(subject: string, kind: string) {
    return sqlite.prepare("SELECT amount FROM usage WHERE subject = ? AND kind = ?").get(subject, kind)?.amount ?? 0;
  }

  async function expectLimit(response: Response, message: RegExp) {
    expect(response.status).toBe(429);
    expect(Number(response.headers.get("retry-after"))).toBeGreaterThan(0);
    expect(await response.json()).toMatchObject({ error: { code: "jarvis_limit", message: expect.stringMatching(message) } });
  }

  it("admits only one parallel request at the device cap without charging the rejected request", async () => {
    const upstream = vi.fn(async () => new Response("{}"));
    vi.stubGlobal("fetch", upstream);
    const responses = await Promise.all(Array.from({ length: 8 }, () =>
      call("tts", first, JSON.stringify({ text: "hello" }), { LIMIT_TTS: "1" }),
    ));
    expect(responses.filter((r) => r.ok)).toHaveLength(1);
    await Promise.all(responses.filter((r) => !r.ok).map((r) => expectLimit(r, /устройства/)));
    expect(upstream).toHaveBeenCalledTimes(1);
    expect(used(first, "tts")).toBe(1);
    expect(used("global", "tenge")).toBe(1);
  });

  it("enforces the prospective shared speech spend across devices, and refunds a refusal", async () => {
    const upstream = vi.fn(async () => new Response("{}"));
    vi.stubGlobal("fetch", upstream);
    const caps = { BUDGET_TENGE: "3" };
    const text = JSON.stringify({ text: "a".repeat(251) }); // two units
    const [one, two] = await Promise.all([call("tts", first, text, caps), call("tts", second, text, caps)]);
    expect(upstream).toHaveBeenCalledTimes(1);
    expect([one.status, two.status].sort()).toEqual([200, 429]);
    await expectLimit(one.ok ? two : one, /бюджет/);
    const blockedDevice = one.ok ? second : first;
    expect(used(blockedDevice, "tts")).toBe(0);
    expect(used("global", "tenge")).toBe(2);

    upstream.mockResolvedValueOnce(new Response(null, { status: 503 }));
    const small = JSON.stringify({ text: "hello" });
    expect((await call("tts", blockedDevice, small, caps)).status).toBe(503);
    expect(used(blockedDevice, "tts")).toBe(0);
    expect(used("global", "tenge")).toBe(2);
    expect((await call("tts", blockedDevice, small, caps)).status).toBe(200);
    expect(used("global", "tenge")).toBe(3);
  });

  it("allows STT recordings that exactly fill the remaining fractional seconds", async () => {
    const upstream = vi.fn(async (url: string) => url.includes("recognizeFileAsync")
      ? Response.json({ id: "test-operation" })
      : new Response("{}"));
    vi.stubGlobal("fetch", upstream);
    const caps = { LIMIT_STT_SECONDS: "0.3" };
    expect((await call("stt", first, new Uint8Array(3200), caps)).status).toBe(200);
    expect((await call("stt", first, new Uint8Array(6400), caps)).status).toBe(200);
    await expectLimit(await call("stt", first, new Uint8Array(3200), caps), /распознавания/);
    expect(upstream).toHaveBeenCalledTimes(4);
    expect(used(first, "stt")).toBeCloseTo(0.3);
  });

  it("cancels an overlong tool body without charging quota or waiting for EOF", async () => {
    const upstream = vi.fn(async () => new Response("{}"));
    const cancel = vi.fn();
    vi.stubGlobal("fetch", upstream);
    const stream = new ReadableStream({
      start(controller) {
        controller.enqueue(new Uint8Array(64 * 1024));
        controller.enqueue(new Uint8Array(1));
      },
      cancel,
    });
    expect((await call("tools/search", first, stream)).status).toBe(413);
    expect(cancel).toHaveBeenCalledTimes(1);
    expect(upstream).not.toHaveBeenCalled();
    expect(used(first, "chat")).toBe(0);
    expect((await call("tools/search", first, "a".repeat(64 * 1024))).status).toBe(200);
  });

  it("counts in-flight chat spend, then settles a stream at its reported cost", async () => {
    let finish!: () => void;
    const stream = new ReadableStream({
      start(controller) {
        finish = () => {
          controller.enqueue(new TextEncoder().encode('data: {"usage":{"prompt_tokens":10,"completion_tokens":2}}\n\n'));
          controller.close();
        };
      },
    });
    const upstream = vi.fn(async () => Response.json({ usage: { prompt_tokens: 10, completion_tokens: 2 } }));
    upstream.mockResolvedValueOnce(new Response(stream));
    vi.stubGlobal("fetch", upstream);
    const caps = { BUDGET_USD: "0.4" };
    const pending = await call("chat/completions", first, JSON.stringify({ stream: true }), caps);
    expect(pending.status).toBe(200);
    await expectLimit(await call("chat/completions", second, "{}", caps), /бюджет/);
    expect(upstream).toHaveBeenCalledTimes(1);
    finish();
    await pending.text();
    await Promise.all(background);
    expect(used("global", "usd")).toBeCloseTo(0.0000054, 12);
    expect((await call("chat/completions", second, "{}", caps)).status).toBe(200);
    await Promise.all(background);
    expect(used(first, "chat")).toBe(1);
    expect(used(second, "chat")).toBe(1);
  });

  it("admits only one parallel registration at the IP cap", async () => {
    const caps = { LIMIT_REGISTRATIONS: "1" };
    const responses = await Promise.all([call("register", first, "", caps), call("register", second, "", caps)]);
    expect(responses.map((r) => r.status).sort()).toEqual([200, 429]);
    await expectLimit(responses.find((r) => !r.ok)!, /адреса/);
    expect(used("ip:192.0.2.1", "reg")).toBe(1);
  });
});
