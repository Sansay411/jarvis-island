// The MCP catalog in Settings and chat: data lives in `mcpCatalog.json` so
// Rust and TypeScript share one source of truth. Packages are pinned to
// releases at least a week old: `npx -y pkg` alone would run whatever was
// published a minute ago. `{field}` in args, url or a value is replaced by
// what the user typed.

import rawCatalog from "./mcpCatalog.json";

export type CatalogCategory = "computer" | "talk" | "web" | "notes" | "dev" | "home" | "money";

export interface CatalogField {
  key: string;
  label: string;
  hint?: string;
  /** Where to get the value. */
  link?: string;
  placeholder?: string;
  initial?: string;
  /** `env`/`header`: kept in Credential Manager; `arg`/`url`: part of the command or address. */
  target: "env" | "header" | "arg" | "url";
  /** For `env`/`header`: the variable or header name. */
  name?: string;
  /** For headers: the value around the input, e.g. «Bearer {value}». */
  format?: string;
  optional?: boolean;
}

export interface CatalogEntry {
  id: string;
  name: string;
  /** Alternative Russian/English names and keywords for search. */
  aliases?: string[];
  category: CatalogCategory;
  description: string;
  /** What has to be installed on the computer. */
  needs?: "node" | "uv" | "app";
  transport: { type: "stdio"; command: string; args: string[] } | { type: "http"; url: string };
  fields: CatalogField[];
  note?: string;
}

export const CATEGORIES: { id: CatalogCategory | "all"; label: string }[] = [
  { id: "all", label: "Все" },
  { id: "computer", label: "Компьютер" },
  { id: "talk", label: "Общение" },
  { id: "web", label: "Интернет" },
  { id: "notes", label: "Заметки и задачи" },
  { id: "dev", label: "Разработка" },
  { id: "home", label: "Умный дом" },
  { id: "money", label: "Бизнес" },
];

export const NEEDS_LABEL: Record<NonNullable<CatalogEntry["needs"]>, string> = {
  node: "нужен Node.js",
  uv: "нужен uv (Python)",
  app: "нужна программа",
};

export const CATALOG: CatalogEntry[] = rawCatalog as CatalogEntry[];

/** The text a field puts into its place (no trailing slash on addresses). */
export function fill(template: string, values: Record<string, string>): string {
  return template.replace(/\{(\w+)\}/g, (_, k: string) => (values[k] ?? "").trim().replace(/\/+$/, ""));
}

/** The server config and secrets a filled-in catalog entry stands for. */
export function build(entry: CatalogEntry, values: Record<string, string>) {
  const env: Record<string, string> = {};
  const headers: Record<string, string> = {};
  for (const f of entry.fields) {
    const v = (values[f.key] ?? "").trim();
    if (!v) continue;
    if (f.target === "env" && f.name) env[f.name] = v;
    if (f.target === "header" && f.name) headers[f.name] = (f.format ?? "{value}").replace("{value}", v);
  }
  const t = entry.transport;
  const transport =
    t.type === "stdio"
      ? { type: "stdio" as const, command: t.command, args: t.args.map((a) => fill(a, values)), cwd: "" }
      : { type: "http" as const, url: fill(t.url, values) };
  return { transport, secrets: { env, headers } };
}

/** Filled enough to connect. */
export function ready(entry: CatalogEntry, values: Record<string, string>): boolean {
  return entry.fields.every((f) => f.optional || (values[f.key] ?? f.initial ?? "").trim().length > 0);
}
