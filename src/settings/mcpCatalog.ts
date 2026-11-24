// The MCP catalog in Settings: servers checked to exist and to start the way
// they are written here (2026-11-24). Packages are pinned to releases at least
// a week old: `npx -y pkg` alone would run whatever was published a minute ago.
// `{field}` in args, url or a value is replaced by what the user typed.

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

const bearer = (link: string, hint: string, label = "Токен"): CatalogField => ({
  key: "token",
  label,
  hint,
  link,
  target: "header",
  name: "Authorization",
  format: "Bearer {value}",
});

const envKey = (name: string, label: string, hint: string, link: string, placeholder?: string): CatalogField => ({
  key: name,
  label,
  hint,
  link,
  placeholder,
  target: "env",
  name,
});

export const CATALOG: CatalogEntry[] = [
  // ---------- the computer itself ----------
  {
    id: "desktop-commander",
    name: "Терминал и процессы",
    category: "computer",
    description: "Команды в терминале, долгие процессы, поиск и правка файлов, работа с CSV и Excel. Опасные действия я всё равно спрошу.",
    needs: "node",
    transport: { type: "stdio", command: "npx", args: ["-y", "@wonderwhy-er/desktop-commander@0.3.3"] },
    fields: [],
  },
  {
    id: "files",
    name: "Файлы в папке",
    category: "computer",
    description: "Чтение, поиск и запись файлов только внутри выбранной папки.",
    needs: "node",
    transport: { type: "stdio", command: "npx", args: ["-y", "@modelcontextprotocol/server-filesystem@2.3.0", "{path}"] },
    fields: [{ key: "path", label: "Папка", target: "arg", initial: "%USERPROFILE%\\Documents", hint: "Полный путь. %USERPROFILE% раскрывается." }],
  },
  {
    id: "playwright",
    name: "Браузер-робот",
    category: "computer",
    description: "Свой браузер Playwright: открывает сайты, нажимает кнопки, заполняет формы и читает страницы по шагам.",
    needs: "node",
    transport: { type: "stdio", command: "npx", args: ["-y", "@playwright/mcp@0.0.85"] },
    fields: [],
  },
  {
    id: "time",
    name: "Время и часовые пояса",
    category: "computer",
    description: "Точное время в любом городе и перевод между часовыми поясами.",
    needs: "uv",
    transport: { type: "stdio", command: "uvx", args: ["mcp-server-time@2026.11.11"] },
    fields: [],
  },

  // ---------- talking to people ----------
  {
    id: "slack",
    name: "Slack",
    category: "talk",
    description: "Каналы, личные сообщения и поиск по переписке. Отправку сообщения я покажу и подожду «Разрешить».",
    needs: "node",
    transport: { type: "stdio", command: "npx", args: ["-y", "slack-mcp-server@1.3.0", "--transport", "stdio"] },
    fields: [envKey("SLACK_MCP_XOXP_TOKEN", "User OAuth Token", "Создайте приложение Slack, дайте права на чтение и запись и скопируйте токен xoxp-…", "https://api.slack.com/apps", "xoxp-…")],
  },
  {
    id: "zapier",
    name: "Zapier: почта, Telegram, WhatsApp",
    category: "talk",
    description: "Тысячи сервисов через Zapier: отправить письмо в Gmail, сообщение в Telegram или WhatsApp, строку в Google Таблицу.",
    transport: { type: "http", url: "{url}" },
    fields: [
      {
        key: "url",
        label: "Адрес сервера",
        target: "url",
        hint: "На mcp.zapier.com создайте сервер, добавьте нужные действия и скопируйте его адрес.",
        link: "https://mcp.zapier.com",
        placeholder: "https://mcp.zapier.com/api/mcp/s/…/mcp",
      },
    ],
  },

  // ---------- the web ----------
  {
    id: "exa",
    name: "Поиск Exa",
    category: "web",
    description: "Умный поиск по интернету и чтение найденных страниц. Без ключа.",
    transport: { type: "http", url: "https://mcp.exa.ai/mcp" },
    fields: [],
  },
  {
    id: "duckduckgo",
    name: "Поиск DuckDuckGo",
    category: "web",
    description: "Поиск в DuckDuckGo и текст найденных страниц. Без ключа и аккаунта.",
    needs: "uv",
    transport: { type: "stdio", command: "uvx", args: ["duckduckgo-mcp-server@0.7.0"] },
    fields: [],
  },
  {
    id: "fetch",
    name: "Чтение сайтов",
    category: "web",
    description: "Любая страница по ссылке в виде чистого текста, без рекламы и меню.",
    needs: "uv",
    transport: { type: "stdio", command: "uvx", args: ["mcp-server-fetch@2026.11.11"] },
    fields: [],
  },
  {
    id: "youtube",
    name: "Расшифровки YouTube",
    category: "web",
    description: "Текст видео с YouTube по ссылке: пересказать ролик, найти момент, сделать конспект.",
    needs: "node",
    transport: { type: "stdio", command: "npx", args: ["-y", "@kimtaeyoon83/mcp-server-youtube-transcript@0.1.1"] },
    fields: [],
  },
  {
    id: "wikipedia",
    name: "Википедия",
    category: "web",
    description: "Статьи, краткие сводки и факты из Википедии на любом языке.",
    needs: "uv",
    transport: { type: "stdio", command: "uvx", args: ["wikipedia-mcp@2.0.1"] },
    fields: [],
  },
  {
    id: "brave",
    name: "Поиск Brave",
    category: "web",
    description: "Поиск по интернету, новостям, картинкам и видео через Brave Search API.",
    needs: "node",
    transport: { type: "stdio", command: "npx", args: ["-y", "@brave/brave-search-mcp-server@2.1.4"] },
    fields: [envKey("BRAVE_API_KEY", "Ключ API", "Бесплатный тариф есть.", "https://brave.com/search/api/")],
  },
  {
    id: "tavily",
    name: "Поиск Tavily",
    category: "web",
    description: "Поиск для ИИ с готовыми выдержками из источников.",
    needs: "node",
    transport: { type: "stdio", command: "npx", args: ["-y", "tavily-mcp@0.2.22"] },
    fields: [envKey("TAVILY_API_KEY", "Ключ API", "Бесплатные запросы каждый месяц.", "https://app.tavily.com", "tvly-…")],
  },
  {
    id: "firecrawl",
    name: "Firecrawl",
    category: "web",
    description: "Обход целых сайтов, извлечение данных со страниц в таблицу.",
    needs: "node",
    transport: { type: "stdio", command: "npx", args: ["-y", "firecrawl-mcp@3.40.0"] },
    fields: [envKey("FIRECRAWL_API_KEY", "Ключ API", "Ключ в личном кабинете Firecrawl.", "https://www.firecrawl.dev/app/api-keys", "fc-…")],
  },

  // ---------- notes and tasks ----------
  {
    id: "notion",
    name: "Notion",
    category: "notes",
    description: "Страницы и базы Notion: найти, прочитать, создать заметку или строку в базе.",
    needs: "node",
    transport: { type: "stdio", command: "npx", args: ["-y", "@notionhq/notion-mcp-server@2.6.1"] },
    fields: [envKey("NOTION_TOKEN", "Токен интеграции", "Создайте внутреннюю интеграцию и откройте ей доступ к нужным страницам.", "https://www.notion.so/profile/integrations", "ntn_…")],
  },
  {
    id: "obsidian",
    name: "Obsidian",
    category: "notes",
    description: "Ваше хранилище Obsidian: поиск, чтение и дописывание заметок.",
    needs: "uv",
    transport: { type: "stdio", command: "uvx", args: ["mcp-obsidian@0.2.3"] },
    fields: [envKey("OBSIDIAN_API_KEY", "Ключ Local REST API", "Включите плагин Local REST API в Obsidian и скопируйте ключ из его настроек.", "https://github.com/coddingtonbear/obsidian-local-rest-api")],
    note: "Obsidian должен быть открыт.",
  },
  {
    id: "todoist",
    name: "Todoist",
    category: "notes",
    description: "Задачи Todoist голосом: добавить, найти, перенести, закрыть.",
    needs: "node",
    transport: { type: "stdio", command: "npx", args: ["-y", "@abhiz123/todoist-mcp-server@0.1.0"] },
    fields: [envKey("TODOIST_API_TOKEN", "Токен API", "Настройки Todoist → Интеграции → Для разработчиков.", "https://app.todoist.com/app/settings/integrations/developer")],
  },
  {
    id: "memory",
    name: "Граф памяти",
    category: "notes",
    description: "Долговременная память в виде связей: люди, проекты, договорённости.",
    needs: "node",
    transport: { type: "stdio", command: "npx", args: ["-y", "@modelcontextprotocol/server-memory@2.1.1"] },
    fields: [],
  },
  {
    id: "thinking",
    name: "Пошаговое мышление",
    category: "notes",
    description: "Разбор сложной задачи по шагам с проверкой и пересмотром решений.",
    needs: "node",
    transport: { type: "stdio", command: "npx", args: ["-y", "@modelcontextprotocol/server-sequential-thinking@2.1.1"] },
    fields: [],
  },

  // ---------- development ----------
  {
    id: "github",
    name: "GitHub",
    category: "dev",
    description: "Репозитории, задачи, пулреквесты и код на GitHub. Официальный сервер.",
    transport: { type: "http", url: "https://api.githubcopilot.com/mcp/" },
    fields: [bearer("https://github.com/settings/personal-access-tokens", "Fine-grained personal access token с доступом к нужным репозиториям.", "Personal access token")],
  },
  {
    id: "git",
    name: "Git в проекте",
    category: "dev",
    description: "История, изменения, ветки и коммиты одного локального репозитория.",
    needs: "uv",
    transport: { type: "stdio", command: "uvx", args: ["mcp-server-git@2026.11.11", "--repository", "{path}"] },
    fields: [{ key: "path", label: "Папка репозитория", target: "arg", placeholder: "E:\\Projects\\my-app" }],
  },
  {
    id: "context7",
    name: "Context7: документация",
    category: "dev",
    description: "Свежая документация любых библиотек и фреймворков прямо в ответе.",
    needs: "node",
    transport: { type: "stdio", command: "npx", args: ["-y", "@upstash/context7-mcp@4.7.1"] },
    fields: [],
  },
  {
    id: "deepwiki",
    name: "DeepWiki",
    category: "dev",
    description: "Объяснит устройство любого открытого репозитория GitHub. Без ключа.",
    transport: { type: "http", url: "https://mcp.deepwiki.com/mcp" },
    fields: [],
  },
  {
    id: "mslearn",
    name: "Microsoft Learn",
    category: "dev",
    description: "Официальная документация Microsoft: Windows, .NET, Azure, PowerShell.",
    transport: { type: "http", url: "https://learn.microsoft.com/api/mcp" },
    fields: [],
  },
  {
    id: "cloudflare-docs",
    name: "Документация Cloudflare",
    category: "dev",
    description: "Ответы по Workers, Pages, R2, D1 и остальной платформе Cloudflare.",
    transport: { type: "http", url: "https://docs.mcp.cloudflare.com/mcp" },
    fields: [],
  },

  // ---------- the home ----------
  {
    id: "home-assistant",
    name: "Home Assistant",
    category: "home",
    description: "Свет, розетки, климат, шторы и сценарии умного дома голосом.",
    transport: { type: "http", url: "{url}/api/mcp" },
    fields: [
      { key: "url", label: "Адрес Home Assistant", target: "url", initial: "http://homeassistant.local:8123", hint: "Включите интеграцию Model Context Protocol Server в Home Assistant." },
      bearer("https://www.home-assistant.io/docs/authentication/#your-account-profile", "Профиль → Безопасность → Долгосрочные токены доступа.", "Токен доступа"),
    ],
  },

  // ---------- business ----------
  {
    id: "stripe",
    name: "Stripe",
    category: "money",
    description: "Платежи, клиенты, подписки и счета в Stripe. Официальный сервер.",
    transport: { type: "http", url: "https://mcp.stripe.com" },
    fields: [bearer("https://dashboard.stripe.com/apikeys", "Создайте ключ с меткой Agent и только нужными правами.", "Agent API key")],
  },
];

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
