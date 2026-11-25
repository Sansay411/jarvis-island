// Jarvis Cloud: the free tier every fresh install starts on. Holds the
// DeepSeek and Yandex SpeechKit keys as Worker secrets, hands each device a
// signed token, and forwards chat, web tools, speech synthesis and speech
// recognition with per-device and service-wide daily limits.

import {
  chatCost, day, issueToken, overLimit, secondsToMidnight, sttText, sttUnits, ttsUnits, usageFromSse, verifyToken,
  type Counters, type Kind, type Limits, type Prices, type TokenUsage,
} from "./core";

const DEEPSEEK = "https://api.deepseek.com";
const SPEECHKIT_TTS = "https://tts.api.ml.yandexcloud.kz/tts/v3/utteranceSynthesis";
const SPEECHKIT_STT = "https://stt.api.ml.yandexcloud.kz/stt/v3";
const MODELS = ["deepseek-flash"];
const MAX_OUTPUT = 8192;
// DeepSeek Flash's advertised 1M-token context, rounded up for reservation.
const MAX_PROMPT_TOKENS = 1_048_576;
const MAX_CHAT_BODY = 12 * 1024 * 1024;
const MAX_TOOL_BODY = 64 * 1024;
const MAX_TTS_CHARS = 1200;
/** 30 s of 16 kHz mono LINEAR16. */
const MAX_STT_BYTES = 30 * 32_000;

const num = (v: string, fallback: number) => (Number.isFinite(Number(v)) && v !== "" ? Number(v) : fallback);

function limits(env: Env): Limits {
  return {
    chat: num(env.LIMIT_CHAT, 300),
    tts: num(env.LIMIT_TTS, 300),
    sttSeconds: num(env.LIMIT_STT_SECONDS, 1200),
    registrations: num(env.LIMIT_REGISTRATIONS, 5),
    usd: num(env.BUDGET_USD, 3),
    tenge: num(env.BUDGET_TENGE, 1500),
  };
}

function prices(env: Env): Prices {
  return {
    inMiss: num(env.PRICE_IN_MISS, 0.3),
    inHit: num(env.PRICE_IN_HIT, 0.006),
    out: num(env.PRICE_OUT, 1.2),
    tool: num(env.PRICE_TOOL, 0.005),
    tts: num(env.PRICE_TTS, 1),
    stt15: num(env.PRICE_STT_15S, 1.5),
  };
}

function json(body: unknown, status = 200, headers: HeadersInit = {}): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json; charset=utf-8", ...headers },
  });
}

/** OpenAI-style error; `jarvis_limit` tells the app not to retry. */
function fail(status: number, message: string, code = "jarvis_error", headers: HeadersInit = {}): Response {
  return json({ error: { message, code } }, status, headers);
}

function readCounters(db: D1Database, today: string, subjects: string[]): D1PreparedStatement {
  const marks = subjects.map(() => "?").join(",");
  return db
    .prepare(`SELECT subject, kind, amount FROM usage WHERE day = ? AND subject IN (${marks})`)
    .bind(today, ...subjects);
}

type UsageRow = { subject: string; kind: Kind; amount: number };
type Charge = [subject: string, kind: Kind, amount: number];
type RequestCharges = [[string, "chat" | "tts" | "stt", number], ["global", "usd" | "tenge", number]];

function reserve(db: D1Database, today: string, items: [...Charge, cap: number][]): D1PreparedStatement {
  const values = items.map(() => "(?, ?, ?, ?, ?)").join(", ");
  return db.prepare(`
    WITH requested(day, subject, kind, amount, cap) AS (VALUES ${values})
    INSERT INTO usage (day, subject, kind, amount)
    SELECT day, subject, kind, amount FROM requested
    WHERE NOT EXISTS (
      SELECT 1 FROM requested r
      LEFT JOIN usage u ON u.day = r.day AND u.subject = r.subject AND u.kind = r.kind
      WHERE COALESCE(u.amount, 0) >= r.cap OR COALESCE(u.amount, 0) + r.amount > r.cap
    )
    ON CONFLICT (day, subject, kind) DO UPDATE SET amount = usage.amount + excluded.amount
  `).bind(...items.flatMap((item) => [today, ...item]));
}

async function adjust(db: D1Database, today: string, items: Charge[]): Promise<void> {
  const stmt = db.prepare("UPDATE usage SET amount = amount + ? WHERE day = ? AND subject = ? AND kind = ?");
  await db.batch(items.map(([s, k, n]) => stmt.bind(n, today, s, k)));
}

async function device(req: Request, env: Env): Promise<string | null> {
  const auth = req.headers.get("authorization") ?? "";
  const token = auth.startsWith("Bearer ") ? auth.slice(7).trim() : "";
  return token ? verifyToken(env.TOKEN_SECRET, token) : null;
}

async function register(req: Request, env: Env, now: Date): Promise<Response> {
  const ip = req.headers.get("cf-connecting-ip") ?? "unknown";
  const today = day(now);
  const subject = `ip:${ip}`;
  const r = await reserve(env.DB, today, [[subject, "reg", 1, limits(env).registrations]]).run();
  if (!r.meta.changes) {
    return fail(429, "Слишком много новых устройств с этого адреса сегодня.", "jarvis_limit", {
      "retry-after": String(secondsToMidnight(now)),
    });
  }
  const id = crypto.randomUUID();
  return json({ token: await issueToken(env.TOKEN_SECRET, id) });
}

async function status(id: string, env: Env, now: Date): Promise<Response> {
  const c: Counters = {};
  for (const r of (await readCounters(env.DB, day(now), [id]).all<UsageRow>()).results) c[r.kind] = r.amount;
  const l = limits(env);
  return json({
    day: day(now),
    resetsIn: secondsToMidnight(now),
    chat: { used: c.chat ?? 0, limit: l.chat },
    tts: { used: c.tts ?? 0, limit: l.tts },
    sttSeconds: { used: Math.round(c.stt ?? 0), limit: l.sttSeconds },
  });
}

async function reserveRequest(env: Env, now: Date, charges: RequestCharges): Promise<Response | null> {
  const [[id, kind, units], [global, currency, cost]] = charges;
  const l = limits(env);
  const today = day(now);
  const [snapshot, reservation] = await env.DB.batch<UsageRow>([
    readCounters(env.DB, today, [id, global]),
    reserve(env.DB, today, [
      [id, kind, units, { chat: l.chat, tts: l.tts, stt: l.sttSeconds }[kind]],
      [global, currency, cost, currency === "usd" ? l.usd : l.tenge],
    ]),
  ]);
  if (!snapshot || !reservation) throw new Error("Missing quota reservation result.");
  if (reservation.meta.changes === 2) return null;
  const own: Counters = {};
  const shared: Counters = {};
  for (const r of snapshot.results) (r.subject === id ? own : shared)[r.kind] = r.amount;
  const why = overLimit(kind, own, shared, l, units, cost);
  if (!why) throw new Error("Quota reservation failed without a limit being reached.");
  return fail(429, why, "jarvis_limit", { "retry-after": String(secondsToMidnight(now)) });
}

function refund(db: D1Database, today: string, charges: Charge[]): Promise<void> {
  return adjust(db, today, charges.map(([s, k, n]) => [s, k, -n]));
}

async function chat(req: Request, env: Env, ctx: ExecutionContext, id: string, now: Date): Promise<Response> {
  const size = Number(req.headers.get("content-length") ?? 0);
  if (size > MAX_CHAT_BODY) return fail(413, "Запрос к модели слишком большой.");
  let body: Record<string, unknown>;
  try {
    body = await req.json();
  } catch {
    return fail(400, "Некорректный JSON.");
  }
  if (!MODELS.includes(String(body.model))) body.model = MODELS[0];
  const asked = Number(body.max_tokens ?? body.max_completion_tokens ?? MAX_OUTPUT);
  const maxTokens = Math.min(Number.isFinite(asked) && asked > 0 ? asked : MAX_OUTPUT, MAX_OUTPUT);
  body.max_tokens = maxTokens;
  delete body.max_completion_tokens;
  delete body.n;
  const stream = body.stream === true;
  if (stream) body.stream_options = { include_usage: true };

  const p = prices(env);
  const reserved = chatCost({ prompt_tokens: MAX_PROMPT_TOKENS, completion_tokens: Math.ceil(maxTokens) }, {
    ...p, inMiss: Math.max(p.inHit, p.inMiss, 0), out: Math.max(p.out, 0),
  });
  const today = day(now);
  const charges: RequestCharges = [[id, "chat", 1], ["global", "usd", reserved]];
  const blocked = await reserveRequest(env, now, charges);
  if (blocked) return blocked;
  const upstream = await fetch(`${DEEPSEEK}/v1/chat/completions`, {
    method: "POST",
    headers: { authorization: `Bearer ${env.DEEPSEEK_KEY}`, "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!upstream.ok) {
    await refund(env.DB, today, charges);
    console.log(JSON.stringify({ event: "deepseek_refused", status: upstream.status }));
    const code = upstream.status === 402 ? 503 : upstream.status;
    return fail(code, code === 503 ? "Jarvis Cloud временно недоступен." : `Модель ответила ошибкой ${upstream.status}.`);
  }
  const settle = async (u: TokenUsage | null) => {
    if (!u || ![u.prompt_tokens, u.completion_tokens].every((n) => typeof n === "number" && Number.isSafeInteger(n) && n >= 0)) return;
    const actual = chatCost(u, p);
    if (Number.isFinite(actual) && actual >= 0) {
      await adjust(env.DB, today, [["global", "usd", actual - reserved]]);
    }
  };
  if (!stream || !upstream.body) {
    const text = await upstream.text();
    let u: TokenUsage | null = null;
    try {
      u = (JSON.parse(text) as { usage?: TokenUsage }).usage ?? null;
    } catch {
      // passed through as is
    }
    ctx.waitUntil(settle(u));
    return new Response(text, { headers: { "content-type": "application/json" } });
  }
  const [client, meter] = upstream.body.tee();
  ctx.waitUntil(
    (async () => {
      // Only the tail matters: the usage chunk comes last.
      const reader = meter.pipeThrough(new TextDecoderStream()).getReader();
      let tail = "";
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        tail = (tail + value).slice(-16_384);
      }
      await settle(usageFromSse(tail));
    })(),
  );
  return new Response(client, {
    headers: { "content-type": "text/event-stream; charset=utf-8", "cache-control": "no-cache" },
  });
}

async function toolBody(req: Request): Promise<string | null> {
  if (Number(req.headers.get("content-length")) > MAX_TOOL_BODY) return null;
  if (!req.body) return "";
  const reader = req.body.getReader();
  const bytes = new Uint8Array(MAX_TOOL_BODY);
  let size = 0;
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) return new TextDecoder().decode(bytes.subarray(0, size));
      if (size + value.byteLength > MAX_TOOL_BODY) {
        void reader.cancel().catch(() => {});
        return null;
      }
      bytes.set(value, size);
      size += value.byteLength;
    }
  } finally {
    reader.releaseLock();
  }
}

async function tool(req: Request, env: Env, id: string, name: string, now: Date): Promise<Response> {
  if (name !== "search" && name !== "fetch") return fail(404, "Нет такого инструмента.");
  const body = await toolBody(req);
  if (body === null) return fail(413, "Запрос слишком большой.");
  const blocked = await reserveRequest(env, now, [[id, "chat", 1], ["global", "usd", Math.max(0, prices(env).tool)]]);
  if (blocked) return blocked;
  const upstream = await fetch(`${DEEPSEEK}/v1/tools/${name}`, {
    method: "POST",
    headers: { authorization: `Bearer ${env.DEEPSEEK_KEY}`, "content-type": "application/json" },
    body,
  });
  return new Response(upstream.body, {
    status: upstream.status === 401 || upstream.status === 402 ? 503 : upstream.status,
    headers: { "content-type": upstream.headers.get("content-type") ?? "application/json" },
  });
}

interface TtsBody { text?: unknown; [k: string]: unknown }

async function tts(req: Request, env: Env, id: string, now: Date): Promise<Response> {
  let body: TtsBody;
  try {
    body = await req.json();
  } catch {
    return fail(400, "Некорректный JSON.");
  }
  const text = typeof body.text === "string" ? body.text : "";
  if (!text.trim()) return fail(400, "Пустой текст.");
  if ([...text].length > MAX_TTS_CHARS) return fail(413, "Слишком длинная фраза для озвучки.");
  const charges: RequestCharges = [[id, "tts", 1], ["global", "tenge", ttsUnits(text) * Math.max(0, prices(env).tts)]];
  const blocked = await reserveRequest(env, now, charges);
  if (blocked) return blocked;
  const upstream = await fetch(SPEECHKIT_TTS, {
    method: "POST",
    headers: { authorization: `Api-Key ${env.YANDEX_KEY}`, "content-type": "application/json" },
    body: JSON.stringify({ ...body, unsafeMode: [...text].length > 240 }),
  });
  if (!upstream.ok) {
    await refund(env.DB, day(now), charges);
    console.log(JSON.stringify({ event: "speechkit_tts_refused", status: upstream.status }));
    return fail(upstream.status === 429 ? 429 : 503, "Голос Яндекса сейчас недоступен.");
  }
  return new Response(upstream.body, { headers: { "content-type": "application/json" } });
}

function b64(bytes: Uint8Array): string {
  let s = "";
  for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(s);
}

/** Raw 16 kHz mono LINEAR16 in, `{ text }` out. SpeechKit in Kazakhstan has
 *  no synchronous REST recognition, so this is the async API polled briskly. */
async function stt(req: Request, env: Env, id: string, now: Date): Promise<Response> {
  const started = Date.now();
  const audio = new Uint8Array(await req.arrayBuffer());
  if (audio.length < 3200) return fail(400, "Слишком короткая запись.");
  if (audio.length > MAX_STT_BYTES) return fail(413, "Запись длиннее 30 секунд.");
  const seconds = audio.length / 32_000;
  const charges: RequestCharges = [[id, "stt", seconds], ["global", "tenge", sttUnits(seconds) * Math.max(0, prices(env).stt15)]];
  const blocked = await reserveRequest(env, now, charges);
  if (blocked) return blocked;
  const headers = { authorization: `Api-Key ${env.YANDEX_KEY}`, "content-type": "application/json" };
  const op = await fetch(`${SPEECHKIT_STT}/recognizeFileAsync`, {
    method: "POST",
    headers,
    body: JSON.stringify({
      content: b64(audio),
      recognitionModel: {
        model: "general",
        audioFormat: { rawAudio: { audioEncoding: "LINEAR16_PCM", sampleRateHertz: "16000", audioChannelCount: "1" } },
        textNormalization: { textNormalization: "TEXT_NORMALIZATION_ENABLED", literatureText: true },
        languageRestriction: { restrictionType: "WHITELIST", languageCode: ["ru-RU", "en-US"] },
      },
    }),
  });
  if (!op.ok) {
    await refund(env.DB, day(now), charges);
    console.log(JSON.stringify({ event: "speechkit_stt_refused", status: op.status }));
    return fail(op.status === 429 ? 429 : 503, "Распознавание Яндекса сейчас недоступно.");
  }
  const { id: opId } = (await op.json()) as { id?: string };
  if (!opId) return fail(503, "Распознавание Яндекса не приняло запись.");
  for (let wait = 150; Date.now() - started < 12_000; wait = Math.min(wait + 50, 400)) {
    await new Promise((r) => setTimeout(r, wait));
    const res = await fetch(`${SPEECHKIT_STT}/getRecognition?operationId=${encodeURIComponent(opId)}`, { headers });
    if (res.ok) return json({ text: sttText(await res.text()), ms: Date.now() - started });
    // 404 with "not ready" until the operation is done.
    await res.body?.cancel();
    if (res.status !== 404 && res.status !== 400) return fail(503, "Распознавание Яндекса ответило ошибкой.");
  }
  return fail(504, "Распознавание Яндекса не успело ответить.");
}

export default {
  async fetch(req, env, ctx): Promise<Response> {
    const url = new URL(req.url);
    const now = new Date();
    const path = url.pathname.replace(/\/+$/, "");
    try {
      if (req.method === "GET" && (path === "" || path === "/health")) return json({ ok: true, service: "jarvis-cloud" });
      if (req.method === "POST" && path === "/v1/register") return register(req, env, now);
      const id = await device(req, env);
      if (!id) return fail(401, "Нет токена устройства Jarvis Cloud.", "jarvis_token");
      if (req.method === "GET" && path === "/v1/status") return status(id, env, now);
      if (req.method === "GET" && path === "/v1/models") return json({ object: "list", data: MODELS.map((m) => ({ id: m, object: "model" })) });
      if (req.method !== "POST") return fail(405, "Метод не поддерживается.");
      if (path === "/v1/chat/completions") return chat(req, env, ctx, id, now);
      if (path.startsWith("/v1/tools/")) return tool(req, env, id, path.slice("/v1/tools/".length), now);
      if (path === "/v1/tts") return tts(req, env, id, now);
      if (path === "/v1/stt") return stt(req, env, id, now);
      return fail(404, "Нет такого адреса.");
    } catch (e) {
      console.log(JSON.stringify({ event: "error", path, message: e instanceof Error ? e.message : String(e) }));
      return fail(500, "Внутренняя ошибка Jarvis Cloud.");
    }
  },
} satisfies ExportedHandler<Env>;
