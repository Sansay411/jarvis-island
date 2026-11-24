// Pure types, formatters and QA fallback fixtures for Settings -> «Навыки».

export type SkillSource = "learned" | "user" | "installed";

export interface SkillSummary {
  slug: string;
  name: string;
  description: string;
  created: string;
  source: SkillSource;
  sourceUrl?: string | null;
  enabled: boolean;
  uses: number;
  lastUsed?: string | null;
}

export interface SkillDetail extends SkillSummary {
  body: string;
}

export interface SkillInstallPreview {
  url: string;
  rawUrl: string;
  slug: string;
  name: string;
  description: string;
  body: string;
  sizeBytes: number;
  exists: boolean;
}

export interface SkillSaveInput {
  slug?: string | null;
  name: string;
  description: string;
  body: string;
  enabled?: boolean;
  source?: SkillSource;
  sourceUrl?: string | null;
}

export const MAX_BODY_BYTES = 16 * 1024;

export const SOURCE_LABEL: Record<SkillSource, string> = {
  learned: "Изучен",
  user: "Свой",
  installed: "Установлен",
};

const utf8Encoder = new TextEncoder();

export function utf8ByteLength(text: string): number {
  return utf8Encoder.encode(text).length;
}

export function formatBytesKb(bytes: number): string {
  if (bytes < 1024) return `${bytes} Б`;
  const kb = (bytes / 1024).toFixed(1).replace(".", ",");
  return `${kb} КБ`;
}

/** Russian pluralization for skill usage counter. */
export function formatUses(count: number): string {
  const n = Math.max(0, Math.floor(count));
  if (n === 0) return "ещё не применялся";
  const mod100 = n % 100;
  const mod10 = n % 10;
  const word = mod100 >= 11 && mod100 <= 14 ? "раз" : mod10 >= 2 && mod10 <= 4 ? "раза" : "раз";
  return `${n} ${word}`;
}

export function formatLastUsed(iso?: string | null, now = new Date()): string | null {
  if (!iso) return null;
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return null;
  const sameDay =
    d.getFullYear() === now.getFullYear() &&
    d.getMonth() === now.getMonth() &&
    d.getDate() === now.getDate();
  if (sameDay) {
    return `сегодня, ${d.toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit" })}`;
  }
  return d.toLocaleDateString("ru-RU", { day: "numeric", month: "short" });
}

export function looksLikeSkillUrl(raw: string): boolean {
  const trimmed = raw.trim();
  if (!trimmed) return false;
  try {
    const u = new URL(trimmed);
    return (u.protocol === "http:" || u.protocol === "https:") && u.hostname.includes(".");
  } catch {
    return false;
  }
}

export function filterSkills<T extends SkillSummary>(skills: T[], query: string): T[] {
  const q = query.trim().toLowerCase().replace(/ё/g, "е");
  if (!q) return skills;
  return skills.filter((s) =>
    `${s.name} ${s.description} ${s.slug}`.toLowerCase().replace(/ё/g, "е").includes(q),
  );
}

/** Sample skills used only in the browser QA showcase when Tauri is absent. */
export const SAMPLE_SKILLS: SkillDetail[] = [
  {
    slug: "vygruzka-otcheta-za-nedelyu",
    name: "Выгрузка отчёта за неделю",
    description: "Когда прошу собрать еженедельный отчёт из папки «Загрузки» и отправить сводку.",
    created: "2026-11-20T09:15:00Z",
    source: "learned",
    enabled: true,
    uses: 6,
    lastUsed: "2026-11-24T08:40:00Z",
    body: [
      "## Цель",
      "Собрать свежий CSV-отчёт за последнюю неделю и подготовить краткую сводку по ключевым цифрам.",
      "",
      "## Шаги",
      "1. Найти в `Downloads` последний файл по шаблону `sales_*.csv` через `files_search`.",
      "2. Прочитать файл, посчитать итог по колонке `amount` и топ-3 категории.",
      "3. Скопировать итоговую папку в `Documents\\Отчёты\\2026`.",
      "4. Вывести короткую сводку в три строки без лишних таблиц.",
      "",
      "## Нюансы",
      "- Разделитель в файле — точка с запятой (`;`), кодировка UTF-8 с BOM.",
    ].join("\n"),
  },
  {
    slug: "podgotovka-rabochego-mesta",
    name: "Подготовка к созвону",
    description: "Перед видеозвонком: приглушить уведомления, открыть заметки и проверить микрофон.",
    created: "2026-11-18T14:00:00Z",
    source: "user",
    enabled: true,
    uses: 3,
    lastUsed: "2026-11-23T15:10:00Z",
    body: [
      "## Цель",
      "Быстро подготовить рабочий стол к звонку с командой.",
      "",
      "## Шаги",
      "1. Сохранить текущую раскладку окон (`window_layout` `snapshot`).",
      "2. Свернуть лишние окна, оставив браузер и заметки (`focus_clean`).",
      "3. Выставить громкость на 60% (`system_volume`).",
    ].join("\n"),
  },
  {
    slug: "git-release-checklist",
    name: "Чек-лист релиза",
    description: "Проверить чистоту рабочей ветки, прогнать тесты и подготовить заметки к релизу.",
    created: "2026-11-12T11:20:00Z",
    source: "installed",
    sourceUrl: "https://raw.githubusercontent.com/example/skills/main/git-release/SKILL.md",
    enabled: false,
    uses: 0,
    lastUsed: null,
    body: [
      "## Цель",
      "Убедиться, что проект собирается и проходит тесты перед тегом.",
      "",
      "## Шаги",
      "1. Проверить `git status` — нет незакоммиченных правок.",
      "2. Запустить тесты и сборку.",
      "3. Сформировать краткий список изменений по коммитам с последнего тега.",
    ].join("\n"),
  },
];

export function buildSamplePreview(url: string, existingSlugs: Set<string>): SkillInstallPreview {
  const rawUrl = url
    .trim()
    .replace("https://github.com/", "https://raw.githubusercontent.com/")
    .replace("/blob/", "/")
    .replace("/tree/", "/");
  const slug = "razbor-pochtovoy-papki";
  const body = [
    "## Цель",
    "Разобрать накопленные письма и выделить задачи на сегодня.",
    "",
    "## Шаги",
    "1. Просмотреть непрочитанные письма за последние 24 часа.",
    "2. Отделить срочные запросы от рассылок.",
    "3. Предложить список из 3–5 дел для добавления в календарь.",
  ].join("\n");
  return {
    url: url.trim(),
    rawUrl: rawUrl.endsWith(".md") ? rawUrl : `${rawUrl.replace(/\/+$/, "")}/SKILL.md`,
    slug,
    name: "Разбор входящей почты",
    description: "Быстро разобрать входящие письма и вытащить задачи в план дня.",
    body,
    sizeBytes: utf8ByteLength(body) + 120,
    exists: existingSlugs.has(slug),
  };
}
