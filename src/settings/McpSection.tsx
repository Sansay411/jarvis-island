// Settings → MCP: servers that lend JARVIS their tools. Changes apply at once
// (there is no save bar here). Env values and headers are written to Windows
// Credential Manager; this page only ever sees their names.

import { AnimatePresence, motion } from "motion/react";
import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { errorMessage } from "../lib/ipc";
import { Group, Row, Segmented, Switch } from "./controls";

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

/** Ready-made servers; the form opens prefilled and editable. */
const PRESETS: { label: string; name: string; transport: Transport; secrets: string; note: string }[] = [
  {
    label: "Файлы",
    name: "Файлы",
    transport: { type: "stdio", command: "npx", args: ["-y", "@modelcontextprotocol/server-filesystem", "%USERPROFILE%\\Documents"], cwd: "" },
    secrets: "",
    note: "Чтение и запись файлов в указанных папках. Нужен Node.js. Замените путь на свою папку; можно указать несколько, по одной на строку.",
  },
  {
    label: "Граф памяти",
    name: "Память",
    transport: { type: "stdio", command: "npx", args: ["-y", "@modelcontextprotocol/server-memory"], cwd: "" },
    secrets: "",
    note: "Долговременная память в виде графа сущностей и связей. Нужен Node.js.",
  },
  {
    label: "GitHub",
    name: "GitHub",
    transport: { type: "http", url: "https://api.githubcopilot.com/mcp/" },
    secrets: "Authorization: Bearer ",
    note: "Официальный удалённый сервер GitHub. Вставьте после «Bearer» свой personal access token (github.com → Settings → Developer settings).",
  },
];

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
  const refresh = useCallback(() => void invoke<ServerView[]>("mcp_list").then(setServers), []);

  useEffect(() => {
    refresh();
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
          <p className="empty">Пока ни одного. Добавьте свой или начните с готового ниже.</p>
        ) : (
          <ul className="mcp-servers">
            {servers.map((s) => (
              <ServerCard key={s.config.id} s={s} notify={notify} onEdit={() => setForm(draftOf(s))} />
            ))}
          </ul>
        )}
      </Group>

      {form === null ? (
        <Group title="Добавить">
          <div className="mcp-add">
            <button className="btn btn-primary" onClick={() => setForm(emptyDraft())}>
              Новый сервер
            </button>
            <button className="btn" onClick={() => setForm("import")}>
              Вставить JSON
            </button>
            <span className="mcp-add-sep" />
            {PRESETS.map((p) => (
              <button
                key={p.label}
                className="btn"
                onClick={() =>
                  setForm({
                    ...emptyDraft(),
                    name: p.name,
                    kind: p.transport.type,
                    command: p.transport.type === "stdio" ? p.transport.command : "npx",
                    args: p.transport.type === "stdio" ? p.transport.args.join("\n") : "",
                    url: p.transport.type === "http" ? p.transport.url : "",
                    secrets: p.secrets,
                    note: p.note,
                  })
                }
              >
                {p.label}
              </button>
            ))}
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
