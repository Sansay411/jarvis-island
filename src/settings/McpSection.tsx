// Settings → MCP: servers that lend JARVIS their tools. Changes apply at once
// (there is no save bar here). Env values and headers are written to Windows
// Credential Manager; this page only ever sees their names.

import { AnimatePresence, motion } from "motion/react";
import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { errorMessage } from "../lib/ipc";
import { hasTauri } from "../lib/workspace-ipc";
import { Group, Row, Segmented, Switch } from "./controls";
import { build, CATALOG, CATEGORIES, NEEDS_LABEL, ready, type CatalogCategory, type CatalogEntry } from "./mcpCatalog";

type Transport = { type: "stdio"; command: string; args: string[]; cwd: string } | { type: "http"; url: string };

interface ServerConfig {
  id: string;
  name: string;
  enabled: boolean;
  transport: Transport;
  envKeys: string[];
  headerKeys: string[];
  disabledTools: string[];
}

interface ToolView {
  name: string;
  title: string;
  description: string;
  risk: "low" | "moderate" | "high";
  enabled: boolean;
}

interface ServerView {
  config: ServerConfig;
  state: "off" | "idle" | "connecting" | "running" | "failed";
  error: string | null;
  serverInfo: string;
  tools: ToolView[];
}

type Tone = "ok" | "error" | "info";

const STATE_LABEL: Record<ServerView["state"], string> = {
  off: "выключен",
  idle: "запустится по требованию",
  connecting: "подключаюсь…",
  running: "работает",
  failed: "ошибка",
};

const RISK_LABEL: Record<ToolView["risk"], string> = {
  low: "только чтение",
  moderate: "спрошу один раз",
  high: "спрошу каждый раз",
};

interface Draft {
  previousId: string | null;
  name: string;
  kind: "stdio" | "http";
  command: string;
  args: string;
  cwd: string;
  url: string;
  /** New secret values; `null` keeps what is stored. */
  secrets: string | null;
  storedKeys: string[];
  enabled: boolean;
  disabledTools: string[];
  note?: string;
}

const emptyDraft = (): Draft => ({
  previousId: null,
  name: "",
  kind: "stdio",
  command: "npx",
  args: "",
  cwd: "",
  url: "",
  secrets: "",
  storedKeys: [],
  enabled: true,
  disabledTools: [],
});

function draftOf(s: ServerView): Draft {
  const t = s.config.transport;
  const keys = t.type === "stdio" ? s.config.envKeys : s.config.headerKeys;
  return {
    previousId: s.config.id,
    name: s.config.name,
    kind: t.type,
    command: t.type === "stdio" ? t.command : "npx",
    args: t.type === "stdio" ? t.args.join("\n") : "",
    cwd: t.type === "stdio" ? t.cwd : "",
    url: t.type === "http" ? t.url : "",
    secrets: keys.length > 0 ? null : "",
    storedKeys: keys,
    enabled: s.config.enabled,
    disabledTools: s.config.disabledTools,
  };
}

/** «KEY=value» (env) or «Name: value» (headers), one per line. */
function parseSecrets(text: string, kind: Draft["kind"]): Record<string, string> {
  const out: Record<string, string> = {};
  for (const raw of text.split("\n")) {
    const line = raw.trim();
    if (!line || line.startsWith("#")) continue;
    const at = kind === "stdio" ? line.indexOf("=") : line.indexOf(":");
    if (at <= 0) throw new Error(kind === "stdio" ? `Строка «${line}»: нужен формат ИМЯ=значение` : `Строка «${line}»: нужен формат Имя: значение`);
    const value = line.slice(at + 1).trim();
    if (value) out[line.slice(0, at).trim()] = value;
  }
  return out;
}

function sameOrigin(a: string, b: string): boolean {
  try {
    return new URL(a).origin === new URL(b).origin;
  } catch {
    return false;
  }
}

function where(t: Transport): string {
  return t.type === "stdio" ? [t.command, ...t.args].join(" ") : t.url;
}

function ServerForm({ initial, onDone, notify }: { initial: Draft; onDone: () => void; notify: (tone: Tone, text: string) => void }) {
  const [d, setD] = useState(initial);
  const [saving, setSaving] = useState(false);
  const patch = (p: Partial<Draft>) => setD((x) => ({ ...x, ...p }));

  const save = async () => {
    try {
      const values = d.secrets === null ? null : parseSecrets(d.secrets, d.kind);
      const transport: Transport =
        d.kind === "stdio"
          ? {
              type: "stdio",
              command: d.command.trim(),
              args: d.args
                .split("\n")
                .map((a) => a.trim())
                .filter(Boolean),
              cwd: d.cwd.trim(),
            }
          : { type: "http", url: d.url.trim() };
      const config: ServerConfig = {
        id: d.previousId ?? "",
        name: d.name,
        enabled: d.enabled,
        transport,
        envKeys: [],
        headerKeys: [],
        disabledTools: d.disabledTools,
      };
      const secrets = values === null ? null : d.kind === "stdio" ? { env: values, headers: {} } : { env: {}, headers: values };
      setSaving(true);
      notify("info", d.enabled ? "Запускаю сервер и читаю его инструменты…" : "Сохраняю…");
      await invoke("mcp_save", { config, secrets, previousId: d.previousId });
      notify("ok", "Сервер сохранён");
      onDone();
    } catch (e) {
      notify("error", e instanceof Error ? e.message : errorMessage(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="mcp-form">
      {d.note && <p className="note">{d.note}</p>}
      <Row label="Название">
        <input value={d.name} placeholder="Например, GitHub" onChange={(e) => patch({ name: e.target.value })} />
      </Row>
      <Row label="Где работает" hint="Локальный сервер — программа на этом компьютере. Удалённый — адрес Streamable HTTP.">
        <Segmented
          label="mcp-kind"
          value={d.kind}
          options={[
            { id: "stdio", label: "Локальный" },
            { id: "http", label: "Удалённый" },
          ]}
          onChange={(kind) => patch({ kind, secrets: d.secrets ?? "" })}
        />
      </Row>
      {d.kind === "stdio" ? (
        <>
          <Row label="Команда" hint="npx, uvx, docker или полный путь к программе.">
            <input className="mono" value={d.command} spellCheck={false} onChange={(e) => patch({ command: e.target.value })} />
          </Row>
          <Row label="Аргументы" hint="По одному на строку — пути с пробелами так не ломаются. Переменные вида %USERPROFILE% раскрываются.">
            <textarea className="mono" rows={3} value={d.args} spellCheck={false} onChange={(e) => patch({ args: e.target.value })} />
          </Row>
          <Row label="Рабочая папка" hint="Необязательно. По умолчанию — папка пользователя.">
            <input className="mono" value={d.cwd} spellCheck={false} onChange={(e) => patch({ cwd: e.target.value })} />
          </Row>
        </>
      ) : (
        <Row label="Адрес">
          <input
            className="mono"
            value={d.url}
            spellCheck={false}
            placeholder="https://…/mcp"
            // Saved headers stay with the address they were entered for.
            onChange={(e) => patch({ url: e.target.value, ...(d.secrets === null && !sameOrigin(initial.url, e.target.value) ? { secrets: "" } : {}) })}
          />
        </Row>
      )}
      <Row
        label={d.kind === "stdio" ? "Переменные окружения" : "Заголовки"}
        hint={`${d.kind === "stdio" ? "ИМЯ=значение" : "Имя: значение"}, по одной на строку. Хранятся в диспетчере учётных данных Windows.`}
      >
        {d.secrets === null ? (
          <div className="inline">
            <span className="ok-text">Сохранены: {d.storedKeys.join(", ")}</span>
            <button className="btn" onClick={() => patch({ secrets: "" })}>
              Заменить
            </button>
          </div>
        ) : (
          <>
            <textarea
              className="mono"
              rows={2}
              value={d.secrets}
              spellCheck={false}
              autoComplete="off"
              placeholder={d.kind === "stdio" ? "GITHUB_TOKEN=…" : "Authorization: Bearer …"}
              onChange={(e) => patch({ secrets: e.target.value })}
            />
            {d.kind === "http" && d.storedKeys.length > 0 && !sameOrigin(initial.url, d.url) && (
              <span className="note">Адрес изменился, поэтому сохранённые заголовки не переносятся. Введите их заново.</span>
            )}
          </>
        )}
      </Row>
      <div className="actions mcp-form-actions">
        <button className="btn" disabled={saving} onClick={onDone}>
          Отмена
        </button>
        <button className="btn btn-primary" disabled={saving || !d.name.trim()} onClick={() => void save()}>
          {saving ? "Подключаюсь…" : "Сохранить"}
        </button>
      </div>
    </div>
  );
}

function ImportForm({ onDone, notify }: { onDone: () => void; notify: (tone: Tone, text: string) => void }) {
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const run = async () => {
    setBusy(true);
    notify("info", "Добавляю серверы и читаю их инструменты…");
    try {
      const names = await invoke<string[]>("mcp_import", { text });
      notify("ok", `Добавлено: ${names.join(", ")}`);
      onDone();
    } catch (e) {
      notify("error", errorMessage(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="mcp-form">
      <p className="note">
        Вставьте конфигурацию из Claude Desktop, Cursor или VS Code — объект «mcpServers» целиком. Переменные окружения и заголовки уйдут в диспетчер учётных данных.
      </p>
      <textarea
        className="mono mcp-import"
        rows={8}
        value={text}
        spellCheck={false}
        placeholder={'{\n  "mcpServers": {\n    "time": { "command": "uvx", "args": ["mcp-server-time"] }\n  }\n}'}
        onChange={(e) => setText(e.target.value)}
      />
      <div className="actions mcp-form-actions">
        <button className="btn" disabled={busy} onClick={onDone}>
          Отмена
        </button>
        <button className="btn btn-primary" disabled={busy || !text.trim()} onClick={() => void run()}>
          {busy ? "Подключаюсь…" : "Добавить"}
        </button>
      </div>
    </div>
  );
}

function CatalogCard({ entry, installed, notify, onDone }: { entry: CatalogEntry; installed: boolean; notify: (tone: Tone, text: string) => void; onDone: () => void }) {
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [values, setValues] = useState<Record<string, string>>(() => Object.fromEntries(entry.fields.map((f) => [f.key, f.initial ?? ""])));

  const connect = async () => {
    setBusy(true);
    notify("info", `${entry.name}: запускаю и читаю инструменты…`);
    try {
      const { transport, secrets } = build(entry, values);
      const config: ServerConfig = { id: entry.id, name: entry.name, enabled: true, transport, envKeys: [], headerKeys: [], disabledTools: [] };
      await invoke("mcp_save", { config, secrets, previousId: null });
      notify("ok", `${entry.name} подключён`);
      setOpen(false);
      onDone();
    } catch (e) {
      notify("error", errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <li className={`mcp-cat-card${open ? " is-open" : ""}${installed ? " is-installed" : ""}`}>
      <div className="mcp-cat-head">
        <div className="mcp-cat-text">
          <span className="mcp-cat-name">{entry.name}</span>
          <span className="mcp-cat-desc">{entry.description}</span>
          <span className="mcp-cat-tags">
            {entry.fields.length === 0 && <span className="mcp-cat-tag is-free">без ключа</span>}
            {entry.needs && <span className="mcp-cat-tag">{NEEDS_LABEL[entry.needs]}</span>}
            {entry.transport.type === "http" && <span className="mcp-cat-tag">в облаке</span>}
          </span>
        </div>
        {installed ? (
          <span className="mcp-cat-done">Подключён</span>
        ) : (
          <button
            className="btn btn-primary"
            disabled={busy}
            onClick={() => (entry.fields.length === 0 ? void connect() : setOpen((v) => !v))}
          >
            {busy ? "Подключаю…" : entry.fields.length === 0 ? "Подключить" : open ? "Свернуть" : "Подключить"}
          </button>
        )}
      </div>
      {open && !installed && (
        <div className="mcp-cat-form">
          {entry.note && <p className="note">{entry.note}</p>}
          {entry.fields.map((f) => (
            <Row key={f.key} label={f.label} hint={f.hint}>
              <div className="mcp-cat-field">
                <input
                  className="mono"
                  type={f.target === "env" || f.target === "header" ? "password" : "text"}
                  autoComplete="off"
                  spellCheck={false}
                  value={values[f.key] ?? ""}
                  placeholder={f.placeholder}
                  onChange={(e) => setValues((v) => ({ ...v, [f.key]: e.target.value }))}
                />
                {f.link && (
                  <a className="mcp-cat-link" href={f.link} target="_blank" rel="noreferrer">
                    Где взять
                  </a>
                )}
              </div>
            </Row>
          ))}
          <div className="actions mcp-form-actions">
            <button className="btn" disabled={busy} onClick={() => setOpen(false)}>
              Отмена
            </button>
            <button className="btn btn-primary" disabled={busy || !ready(entry, values)} onClick={() => void connect()}>
              {busy ? "Подключаю…" : "Подключить"}
            </button>
          </div>
          {(entry.fields.some((f) => f.target === "env" || f.target === "header")) && (
            <p className="note">Ключи хранятся в диспетчере учётных данных Windows и уходят только этому серверу.</p>
          )}
        </div>
      )}
    </li>
  );
}

function Catalog({ servers, notify, onDone }: { servers: ServerView[]; notify: (tone: Tone, text: string) => void; onDone: () => void }) {
  const [cat, setCat] = useState<CatalogCategory | "all">("all");
  const [query, setQuery] = useState("");
  const q = query.trim().toLowerCase();
  const have = new Set(servers.map((s) => s.config.id));
  const list = CATALOG.filter(
    (e) => (cat === "all" || e.category === cat) && (!q || `${e.name} ${(e.aliases ?? []).join(" ")} ${e.description}`.toLowerCase().includes(q)),
  );
  return (
    <div className="mcp-catalog">
      <div className="mcp-cat-bar">
        <input className="mcp-cat-search" value={query} placeholder="Найти: почта, поиск, GitHub, умный дом…" onChange={(e) => setQuery(e.target.value)} />
        <div className="mcp-cat-chips" role="tablist">
          {CATEGORIES.map((c) => (
            <button key={c.id} className={`mcp-cat-chip${cat === c.id ? " is-on" : ""}`} onClick={() => setCat(c.id)}>
              {c.label}
            </button>
          ))}
        </div>
      </div>
      {list.length === 0 ? (
        <p className="empty">Ничего не нашлось. Любой другой сервер можно добавить вручную или вставить его JSON.</p>
      ) : (
        <ul className="mcp-cat-list">
          {list.map((e) => (
            <CatalogCard key={e.id} entry={e} installed={have.has(e.id)} notify={notify} onDone={onDone} />
          ))}
        </ul>
      )}
    </div>
  );
}

function ServerCard({ s, onEdit, notify }: { s: ServerView; onEdit: () => void; notify: (tone: Tone, text: string) => void }) {
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const id = s.config.id;
  const enabledTools = s.tools.filter((t) => t.enabled).length;

  const act = async (f: () => Promise<unknown>, ok?: string) => {
    setBusy(true);
    try {
      await f();
      if (ok) notify("ok", ok);
    } catch (e) {
      notify("error", errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <li className={`mcp-server is-${s.state}`}>
      <div className="mcp-head">
        <button className="mcp-title" aria-expanded={open} onClick={() => setOpen(!open)}>
          <span className={`dot mcp-dot-${s.state}`} />
          <span className="mcp-name">{s.config.name}</span>
          <span className="mcp-meta">
            {STATE_LABEL[s.state]}
            {s.tools.length > 0 && ` · ${enabledTools === s.tools.length ? s.tools.length : `${enabledTools} из ${s.tools.length}`} инстр.`}
          </span>
          <span className="mcp-where mono">{where(s.config.transport)}</span>
        </button>
        <Switch label={`Сервер ${s.config.name}`} checked={s.config.enabled} onChange={(v) => void act(() => invoke("mcp_set_enabled", { id, enabled: v }))} />
      </div>
      {s.error && <div className="callout mcp-error">{s.error}</div>}
      <AnimatePresence initial={false}>
        {open && (
          <motion.div
            className="mcp-body"
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: "auto", opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={{ type: "spring", stiffness: 420, damping: 38 }}
          >
            {s.serverInfo && <p className="note">Сервер: {s.serverInfo}</p>}
            {s.tools.length === 0 ? (
              <p className="empty">Инструменты ещё не прочитаны — нажмите «Проверить».</p>
            ) : (
              <ul className="mcp-tools">
                {s.tools.map((t) => (
                  <li key={t.name}>
                    <div className="mcp-tool-text">
                      <span className="mcp-tool-name">{t.title}</span>
                      <span className={`mcp-risk risk-${t.risk}`}>{RISK_LABEL[t.risk]}</span>
                      {t.description && <span className="mcp-tool-desc">{t.description}</span>}
                    </div>
                    <Switch label={t.title} checked={t.enabled} onChange={(v) => void act(() => invoke("mcp_set_tool", { id, tool: t.name, enabled: v }))} />
                  </li>
                ))}
              </ul>
            )}
            <div className="actions mcp-card-actions">
              <button className="btn btn-danger" disabled={busy} onClick={() => void act(() => invoke("mcp_remove", { id }), "Сервер удалён")}>
                Удалить
              </button>
              <button className="btn" disabled={busy} onClick={onEdit}>
                Изменить
              </button>
              <button
                className="btn"
                disabled={busy || !s.config.enabled}
                onClick={() => void act(() => invoke<number>("mcp_refresh", { id }).then((n) => notify("ok", `${s.config.name}: инструментов — ${n}`)))}
              >
                {busy ? "Проверяю…" : "Проверить"}
              </button>
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </li>
  );
}

export function McpSection({ notify, vy }: { notify: (tone: Tone, text: string) => void; vy: boolean }) {
  const [servers, setServers] = useState<ServerView[] | null>(null);
  const [form, setForm] = useState<Draft | "import" | null>(null);
  const refresh = useCallback(() => {
    if (!hasTauri()) {
      setServers([]);
      return;
    }
    void invoke<ServerView[]>("mcp_list").then(setServers).catch(() => setServers([]));
  }, []);

  useEffect(() => {
    refresh();
    if (!hasTauri()) return;
    const un = listen("mcp:changed", refresh);
    return () => void un.then((f) => f());
  }, [refresh]);

  if (!servers) return <p className="empty">Загружаю…</p>;
  const close = () => (setForm(null), refresh());
  const total = servers.filter((s) => s.config.enabled).reduce((n, s) => n + s.tools.filter((t) => t.enabled).length, 0);

  return (
    <>
      <p className="note">
        MCP — открытый стандарт подключения инструментов: GitHub, Notion, файлы, базы данных и сотни других серверов. Их инструменты я использую так же, как свои, с теми же
        подтверждениями. Локальный сервер запускается, когда {vy ? "вы просите" : "ты просишь"} о его деле, и выключается после десяти минут без работы.
      </p>
      {total > 60 && (
        <div className="callout">
          Включено {total} инструментов MCP. Описание каждого уходит модели в каждом запросе — отключите лишние, чтобы я отвечал быстрее и точнее.
        </div>
      )}

      <Group title="Серверы">
        {servers.length === 0 ? (
          <p className="empty">Пока ни одного. Подключите готовый из каталога ниже в один клик.</p>
        ) : (
          <ul className="mcp-servers">
            {servers.map((s) => (
              <ServerCard key={s.config.id} s={s} notify={notify} onEdit={() => setForm(draftOf(s))} />
            ))}
          </ul>
        )}
      </Group>

      <Group title="Каталог">
        <Catalog servers={servers} notify={notify} onDone={refresh} />
      </Group>

      {form === null ? (
        <Group title="Свой сервер">
          <div className="mcp-add">
            <button className="btn" onClick={() => setForm(emptyDraft())}>
              Новый сервер
            </button>
            <button className="btn" onClick={() => setForm("import")}>
              Вставить JSON
            </button>
          </div>
        </Group>
      ) : (
        <Group title={form === "import" ? "Импорт" : form.previousId ? `Изменить «${form.name}»` : "Новый сервер"}>
          {form === "import" ? <ImportForm onDone={close} notify={notify} /> : <ServerForm initial={form} onDone={close} notify={notify} />}
        </Group>
      )}
    </>
  );
}
