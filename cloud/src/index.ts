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
const MAX_CHAT_BODY = 12 * 1024 * 1024;
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

async function counters(db: D1Database, today: string, subjects: string[]): Promise<Map<string, Counters>> {
  const marks = subjects.map(() => "?").join(",");
  const { results } = await db
    .prepare(`SELECT subject, kind, amount FROM usage WHERE day = ? AND subject IN (${marks})`)
    .bind(today, ...subjects)
    .all<{ subject: string; kind: Kind; amount: number }>();
  const out = new Map<string, Counters>(subjects.map((s) => [s, {}]));
  for (const r of results) out.get(r.subject)![r.kind] = r.amount;
  return out;
}

async function add(db: D1Database, today: string, items: [string, Kind, number][]): Promise<void> {
  const stmt = db.prepare(
    "INSERT INTO usage (day, subject, kind, amount) VALUES (?, ?, ?, ?) ON CONFLICT (day, subject, kind) DO UPDATE SET amount = amount + excluded.amount",
  );
  await db.batch(items.filter(([, , n]) => n > 0).map(([s, k, n]) => stmt.bind(today, s, k, n)));
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
  const c = (await counters(env.DB, today, [subject])).get(subject)!;
  if ((c.reg ?? 0) >= limits(env).registrations) {
    return fail(429, "Слишком много новых устройств с этого адреса сегодня.", "jarvis_limit", {
      "retry-after": String(secondsToMidnight(now)),
    });
  }
  const id = crypto.randomUUID();
  await add(env.DB, today, [[subject, "reg", 1]]);
  return json({ token: await issueToken(env.TOKEN_SECRET, id) });
}

async function status(id: string, env: Env, now: Date): Promise<Response> {
  const c = (await counters(env.DB, day(now), [id])).get(id)!;
  const l = limits(env);
  return json({
    day: day(now),
    resetsIn: secondsToMidnight(now),
    chat: { used: c.chat ?? 0, limit: l.chat },
    tts: { used: c.tts ?? 0, limit: l.tts },
    sttSeconds: { used: Math.round(c.stt ?? 0), limit: l.sttSeconds },
  });
}

/** Refuses with 429 + Retry-After when the device or the service is out. */
async function gate(env: Env, id: string, kind: "chat" | "tts" | "stt", now: Date): Promise<Response | null> {
  const c = await counters(env.DB, day(now), [id, "global"]);
  const why = overLimit(kind, c.get(id)!, c.get("global")!, limits(env));
  return why ? fail(429, why, "jarvis_limit", { "retry-after": String(secondsToMidnight(now)) }) : null;
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
  const blocked = await gate(env, id, "chat", now);
  if (blocked) return blocked;
  if (!MODELS.includes(String(body.model))) body.model = MODELS[0];
  const asked = Number(body.max_tokens ?? body.max_completion_tokens ?? MAX_OUTPUT);
  body.max_tokens = Math.min(Number.isFinite(asked) && asked > 0 ? asked : MAX_OUTPUT, MAX_OUTPUT);
  delete body.max_completion_tokens;
  delete body.n;
  const stream = body.stream === true;
  if (stream) body.stream_options = { include_usage: true };

  const upstream = await fetch(`${DEEPSEEK}/v1/chat/completions`, {
    method: "POST",
    headers: { authorization: `Bearer ${env.DEEPSEEK_KEY}`, "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  const today = day(now);
  if (!upstream.ok) {
    console.log(JSON.stringify({ event: "deepseek_refused", status: upstream.status }));
    const code = upstream.status === 402 ? 503 : upstream.status;
    return fail(code, code === 503 ? "Jarvis Cloud временно недоступен." : `Модель ответила ошибкой ${upstream.status}.`);
  }
  const p = prices(env);
  const charge = (u: TokenUsage | null) =>
    add(env.DB, today, [[id, "chat", 1], ["global", "usd", u ? chatCost(u, p) : 0.002]]);
  if (!stream || !upstream.body) {
    const text = await upstream.text();
    let u: TokenUsage | null = null;
    try {
      u = (JSON.parse(text) as { usage?: TokenUsage }).usage ?? null;
    } catch {
      // passed through as is
    }
    ctx.waitUntil(charge(u));
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
      await charge(usageFromSse(tail));
    })(),
  );
  return new Response(client, {
    headers: { "content-type": "text/event-stream; charset=utf-8", "cache-control": "no-cache" },
  });
}

async function tool(req: Request, env: Env, ctx: ExecutionContext, id: string, name: string, now: Date): Promise<Response> {
  if (name !== "search" && name !== "fetch") return fail(404, "Нет такого инструмента.");
  const blocked = await gate(env, id, "chat", now);
  if (blocked) return blocked;
  const body = await req.text();
  if (body.length > 64 * 1024) return fail(413, "Запрос слишком большой.");
  const upstream = await fetch(`${DEEPSEEK}/v1/tools/${name}`, {
    method: "POST",
    headers: { authorization: `Bearer ${env.DEEPSEEK_KEY}`, "content-type": "application/json" },
    body,
  });
  ctx.waitUntil(add(env.DB, day(now), [[id, "chat", 1], ["global", "usd", prices(env).tool]]));
  return new Response(upstream.body, {
    status: upstream.status === 401 || upstream.status === 402 ? 503 : upstream.status,
    headers: { "content-type": upstream.headers.get("content-type") ?? "application/json" },
  });
}

interface TtsBody { text?: unknown; [k: string]: unknown }

async function tts(req: Request, env: Env, ctx: ExecutionContext, id: string, now: Date): Promise<Response> {
  let body: TtsBody;
  try {
    body = await req.json();
  } catch {
    return fail(400, "Некорректный JSON.");
  }
  const text = typeof body.text === "string" ? body.text : "";
  if (!text.trim()) return fail(400, "Пустой текст.");
  if ([...text].length > MAX_TTS_CHARS) return fail(413, "Слишком длинная фраза для озвучки.");
  const blocked = await gate(env, id, "tts", now);
  if (blocked) return blocked;
  const upstream = await fetch(SPEECHKIT_TTS, {
    method: "POST",
    headers: { authorization: `Api-Key ${env.YANDEX_KEY}`, "content-type": "application/json" },
    body: JSON.stringify({ ...body, unsafeMode: [...text].length > 240 }),
  });
  if (!upstream.ok) {
    console.log(JSON.stringify({ event: "speechkit_tts_refused", status: upstream.status }));
    return fail(upstream.status === 429 ? 429 : 503, "Голос Яндекса сейчас недоступен.");
  }
  const units = ttsUnits(text);
  ctx.waitUntil(add(env.DB, day(now), [[id, "tts", 1], ["global", "tenge", units * prices(env).tts]]));
  return new Response(upstream.body, { headers: { "content-type": "application/json" } });
}

function b64(bytes: Uint8Array): string {
  let s = "";
  for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(s);
}

/** Raw 16 kHz mono LINEAR16 in, `{ text }` out. SpeechKit in Kazakhstan has
 *  no synchronous REST recognition, so this is the async API polled briskly. */
async function stt(req: Request, env: Env, ctx: ExecutionContext, id: string, now: Date): Promise<Response> {
  const started = Date.now();
  const audio = new Uint8Array(await req.arrayBuffer());
  if (audio.length < 3200) return fail(400, "Слишком короткая запись.");
  if (audio.length > MAX_STT_BYTES) return fail(413, "Запись длиннее 30 секунд.");
  const blocked = await gate(env, id, "stt", now);
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
    console.log(JSON.stringify({ event: "speechkit_stt_refused", status: op.status }));
    return fail(op.status === 429 ? 429 : 503, "Распознавание Яндекса сейчас недоступно.");
  }
  const { id: opId } = (await op.json()) as { id?: string };
  if (!opId) return fail(503, "Распознавание Яндекса не приняло запись.");
  const seconds = audio.length / 32_000;
  ctx.waitUntil(add(env.DB, day(now), [[id, "stt", seconds], ["global", "tenge", sttUnits(seconds) * prices(env).stt15]]));
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
      if (path.startsWith("/v1/tools/")) return tool(req, env, ctx, id, path.slice("/v1/tools/".length), now);
      if (path === "/v1/tts") return tts(req, env, ctx, id, now);
      if (path === "/v1/stt") return stt(req, env, ctx, id, now);
      return fail(404, "Нет такого адреса.");
    } catch (e) {
      console.log(JSON.stringify({ event: "error", path, message: e instanceof Error ? e.message : String(e) }));
      return fail(500, "Внутренняя ошибка Jarvis Cloud.");
    }
  },
} satisfies ExportedHandler<Env>;
