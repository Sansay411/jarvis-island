// Pure helpers of Jarvis Cloud: device tokens, prices, limits, the
// SpeechKit answer. No Workers globals, so they run under Node tests too.

export const TOKEN_PREFIX = "j1";

const enc = new TextEncoder();

function b64url(bytes: ArrayBuffer): string {
  let s = "";
  for (const b of new Uint8Array(bytes)) s += String.fromCharCode(b);
  return btoa(s).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

async function hmac(secret: string, data: string): Promise<ArrayBuffer> {
  const key = await crypto.subtle.importKey("raw", enc.encode(secret), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  return crypto.subtle.sign("HMAC", key, enc.encode(data));
}

/** `j1.<device>.<signature>`: checked without a database lookup. */
export async function issueToken(secret: string, device: string): Promise<string> {
  return `${TOKEN_PREFIX}.${device}.${b64url(await hmac(secret, device))}`;
}

/** The device id of a valid token, or null. */
export async function verifyToken(secret: string, token: string): Promise<string | null> {
  const [prefix, device, sig] = token.split(".");
  if (prefix !== TOKEN_PREFIX || !device || !sig || !/^[0-9a-f-]{36}$/.test(device)) return null;
  const want = enc.encode(b64url(await hmac(secret, device)));
  const got = enc.encode(sig);
  if (want.length !== got.length) return null;
  let diff = 0;
  for (let i = 0; i < want.length; i++) diff |= want[i]! ^ got[i]!;
  return diff === 0 ? device : null;
}

export interface Limits {
  /** Per device and UTC day. */
  chat: number;
  tts: number;
  sttSeconds: number;
  /** Per IP and day: new devices. */
  registrations: number;
  /** Whole service per day. */
  usd: number;
  tenge: number;
}

export interface Prices {
  /** USD per 1M tokens (DeepSeek peak prices). */
  inMiss: number;
  inHit: number;
  out: number;
  /** One web search or page read. */
  tool: number;
  /** Tenge per TTS request of up to 250 characters. */
  tts: number;
  /** Tenge per started 15 s of recognised audio. */
  stt15: number;
}

export interface TokenUsage {
  prompt_tokens?: number;
  completion_tokens?: number;
  prompt_cache_hit_tokens?: number;
  prompt_tokens_details?: { cached_tokens?: number };
}

export function chatCost(u: TokenUsage, p: Prices): number {
  const prompt = u.prompt_tokens ?? 0;
  const hit = Math.min(prompt, u.prompt_cache_hit_tokens ?? u.prompt_tokens_details?.cached_tokens ?? 0);
  return ((prompt - hit) * p.inMiss + hit * p.inHit + (u.completion_tokens ?? 0) * p.out) / 1e6;
}

export const ttsUnits = (text: string) => Math.max(1, Math.ceil([...text].length / 250));
export const sttUnits = (seconds: number) => Math.max(1, Math.ceil(seconds / 15));

/** The last `usage` object in an SSE stream of chat chunks. */
export function usageFromSse(sse: string): TokenUsage | null {
  let found: TokenUsage | null = null;
  for (const line of sse.split("\n")) {
    if (!line.startsWith("data:") || !line.includes('"usage"')) continue;
    try {
      const u = (JSON.parse(line.slice(5)) as { usage?: TokenUsage | null }).usage;
      if (u) found = u;
    } catch {
      // a partial or foreign line
    }
  }
  return found;
}

export type Kind = "chat" | "tts" | "stt" | "reg" | "usd" | "tenge";
export type Counters = Partial<Record<Kind, number>>;

/** What blocks a request, or null when it may go. */
export function overLimit(kind: "chat" | "tts" | "stt", device: Counters, global: Counters, l: Limits, units = 0, cost = 0): string | null {
  const own = { chat: l.chat, tts: l.tts, stt: l.sttSeconds }[kind];
  const used = device[kind] ?? 0;
  if (used >= own || used + units > own) {
    const what = { chat: "запросов к модели", tts: "озвучек", stt: "распознавания речи" }[kind];
    return `Дневной лимит Jarvis Cloud для этого устройства (${what}) исчерпан. Он обновится в 00:00 UTC, или добавьте свой ключ в настройках.`;
  }
  const spent = (kind === "chat" ? global.usd : global.tenge) ?? 0;
  const budget = kind === "chat" ? l.usd : l.tenge;
  return spent >= budget || spent + cost > budget ? "Общий дневной бюджет Jarvis Cloud на сегодня исчерпан. Он обновится в 00:00 UTC, или добавьте свой ключ в настройках." : null;
}

export const day = (now: Date) => now.toISOString().slice(0, 10);

export function secondsToMidnight(now: Date): number {
  const next = Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate() + 1);
  return Math.max(1, Math.ceil((next - now.getTime()) / 1000));
}

interface Alternative { text?: string }
interface SttMessage {
  channelTag?: string;
  final?: { alternatives?: Alternative[]; channelTag?: string };
  finalRefinement?: { normalizedText?: { alternatives?: Alternative[] } };
}

/** Text of a SpeechKit v3 recognition answer (one JSON object per line).
 *  The normalised refinement of each utterance wins over its raw final. */
export function sttText(body: string): string {
  const raw: string[] = [];
  const refined: string[] = [];
  for (const line of body.split("\n")) {
    if (!line.trim()) continue;
    let m: { result?: SttMessage } & SttMessage;
    try {
      m = JSON.parse(line);
    } catch {
      continue;
    }
    const r = m.result ?? m;
    const f = r.finalRefinement?.normalizedText?.alternatives?.[0]?.text;
    if (f !== undefined) refined.push(f);
    const t = r.final?.alternatives?.[0]?.text;
    if (t !== undefined) raw.push(t);
  }
  return (refined.length ? refined : raw).map((s) => s.trim()).filter(Boolean).join(" ");
}
