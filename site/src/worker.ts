// The landing's only server code. Static pages come from public/ without
// running this; it answers /download/* (a redirect to the newest release
// asset) and /api/waitlist (Pro launch e-mails).

const PLATFORMS = {
  windows: /-Setup\.exe$/,
  "mac-arm": /_aarch64\.dmg$/,
  "mac-intel": /_x64\.dmg$/,
  portable: /^jarvis-portable\.exe$/,
} as const;
type Platform = keyof typeof PLATFORMS;

const EMAIL = /^[^\s@]+@[^\s@]+\.[^\s@]{2,}$/;
const RELEASE_TTL = 600;

interface Release {
  html_url: string;
  assets: { name: string; browser_download_url: string }[];
}

const json = (body: unknown, status = 200) =>
  new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json; charset=utf-8" } });

const today = () => new Date().toISOString().slice(0, 10);

async function latestRelease(env: Env, ctx: ExecutionContext): Promise<Release | null> {
  const api = `https://api.github.com/repos/${env.GITHUB_REPO}/releases/latest`;
  const cache = caches.default;
  const hit = await cache.match(api);
  if (hit) return hit.json();
  const res = await fetch(api, { headers: { "user-agent": "jarvis-site", accept: "application/vnd.github+json" } });
  if (!res.ok) return null;
  const body = await res.text();
  ctx.waitUntil(
    cache.put(api, new Response(body, { headers: { "content-type": "application/json", "cache-control": `max-age=${RELEASE_TTL}` } })),
  );
  return JSON.parse(body);
}

async function download(platform: string, env: Env, ctx: ExecutionContext): Promise<Response> {
  const releases = `https://github.com/${env.GITHUB_REPO}/releases/latest`;
  if (!(platform in PLATFORMS)) return Response.redirect(releases, 302);
  const release = await latestRelease(env, ctx);
  const asset = release?.assets.find((a) => PLATFORMS[platform as Platform].test(a.name));
  ctx.waitUntil(
    env.DB.prepare(
      "INSERT INTO downloads (day, platform, count) VALUES (?, ?, 1) ON CONFLICT (day, platform) DO UPDATE SET count = count + 1",
    )
      .bind(today(), platform)
      .run()
      .catch(() => undefined),
  );
  return Response.redirect(asset?.browser_download_url ?? release?.html_url ?? releases, 302);
}

async function waitlist(request: Request, env: Env): Promise<Response> {
  if (request.method !== "POST") return json({ ok: false }, 405);
  const body = (await request.json().catch(() => null)) as Record<string, unknown> | null;
  const email = typeof body?.email === "string" ? body.email.trim().toLowerCase() : "";
  // A hidden field people never fill in; bots do.
  if (body?.website) return json({ ok: true });
  if (email.length > 254 || !EMAIL.test(email)) return json({ ok: false, error: "email" }, 400);
  const plan = body?.plan === "yearly" ? "yearly" : "monthly";
  const lang = body?.lang === "en" ? "en" : "ru";
  const country = (request.cf?.country as string | undefined) ?? null;
  await env.DB.prepare("INSERT OR IGNORE INTO waitlist (email, plan, lang, country, created_at) VALUES (?, ?, ?, ?, ?)")
    .bind(email, plan, lang, country, new Date().toISOString())
    .run();
  return json({ ok: true });
}

export default {
  async fetch(request, env, ctx): Promise<Response> {
    const { pathname } = new URL(request.url);
    if (pathname.startsWith("/download/")) return download(pathname.slice("/download/".length), env, ctx);
    if (pathname === "/api/waitlist") return waitlist(request, env);
    if (pathname.startsWith("/api/")) return json({ ok: false }, 404);
    return env.ASSETS.fetch(request);
  },
} satisfies ExportedHandler<Env>;
