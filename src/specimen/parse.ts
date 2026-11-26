// Reads design tokens and icon names straight from their source files so the
// specimen page never drifts from what the app actually ships.

export type TokenKind = "color" | "font" | "size" | "duration" | "easing" | "other";

export interface Token {
  name: string;
  value: string;
  group: string;
  note?: string;
}

const COLOR_RE = /^(#[0-9a-f]{3,8}|rgba?\(|hsla?\(|transparent$)/i;

/** Custom properties of the first `:root` block, grouped by the comment above them. */
export function parseTokens(css: string): Token[] {
  const start = css.indexOf(":root");
  if (start < 0) return [];
  const open = css.indexOf("{", start);
  const close = css.indexOf("\n}", open);
  const body = css.slice(open + 1, close < 0 ? undefined : close);

  const tokens: Token[] = [];
  let group = "Tokens";
  let note: string | undefined;
  const re = /\/\*([\s\S]*?)\*\/|(--[\w-]+)\s*:\s*([^;]+);/g;
  for (let m = re.exec(body); m; m = re.exec(body)) {
    if (m[1] !== undefined) {
      const text = m[1].replace(/\s+/g, " ").trim();
      const multi = m[1].includes("\n");
      if (multi) {
        group = text.split(/[:.]/)[0]!.trim();
        note = text;
      } else {
        group = text;
        note = undefined;
      }
      continue;
    }
    tokens.push({ name: m[2]!, value: m[3]!.replace(/\s+/g, " ").trim(), group, ...(note ? { note } : {}) });
  }
  return tokens;
}

/** Follows `var(--x)` references until a literal value. */
export function resolveToken(value: string, tokens: Token[], depth = 0): string {
  const m = /^var\((--[\w-]+)\)$/.exec(value.trim());
  if (!m || depth > 8) return value;
  const next = tokens.find((t) => t.name === m[1]);
  return next ? resolveToken(next.value, tokens, depth + 1) : value;
}

export function tokenKind(token: Token, tokens: Token[]): TokenKind {
  const v = resolveToken(token.value, tokens);
  if (token.name.startsWith("--font")) return "font";
  if (token.name.startsWith("--dur")) return "duration";
  if (token.name.startsWith("--ease")) return "easing";
  if (/^-?\d+(\.\d+)?px$/.test(v)) return "size";
  if (COLOR_RE.test(v)) return "color";
  return "other";
}

/** Groups in source order. */
export function groupTokens(tokens: Token[]): { group: string; note?: string; tokens: Token[] }[] {
  const out: { group: string; note?: string; tokens: Token[] }[] = [];
  for (const t of tokens) {
    let g = out[out.length - 1];
    if (!g || g.group !== t.group) {
      g = { group: t.group, ...(t.note ? { note: t.note } : {}), tokens: [] };
      out.push(g);
    }
    g.tokens.push(t);
  }
  return out;
}

/** Keys of the `paths` table in `src/ui/Icon.tsx`. */
export function parseIconNames(src: string): string[] {
  const start = src.indexOf("const paths = {");
  if (start < 0) return [];
  const end = src.indexOf("\n};", start);
  const body = src.slice(start, end < 0 ? undefined : end);
  const names: string[] = [];
  const re = /^ {2}([A-Za-z_][\w]*):/gm;
  for (let m = re.exec(body); m; m = re.exec(body)) names.push(m[1]!);
  return names;
}
