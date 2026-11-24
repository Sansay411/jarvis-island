// What JARVIS learned about working with the user (`learn` lessons): how
// each kind reads in Settings → «Память» → «Чему я научился».

export type LessonKind = "preference" | "humor" | "alias" | "hearing";

export interface Lesson {
  id: number;
  kind: LessonKind | string;
  key: string;
  value: string;
  scope: string;
  note: string;
  source: "model" | "auto" | "reflect" | "user" | string;
  hits: number;
  createdAt: string;
  updatedAt: string;
}

export const KIND_ORDER: LessonKind[] = ["preference", "humor", "alias", "hearing"];

export function kindTitle(kind: LessonKind, vy: boolean): string {
  switch (kind) {
    case "preference":
      return vy ? "Как вам удобнее" : "Как тебе удобнее";
    case "humor":
      return "Юмор";
    case "alias":
      return vy ? "Ваши слова" : "Твои слова";
    case "hearing":
      return vy ? "Как я вас слышу" : "Как я тебя слышу";
  }
}

export const SOURCE_LABEL: Record<string, string> = {
  model: "в разговоре",
  auto: "сам заметил",
  reflect: "из разбора разговоров",
  user: "вручную",
};

/** «ночник» → «Bedside lamp» (свет), or the rule itself. */
export function lessonText(l: Lesson): string {
  if (l.kind === "alias" || l.kind === "hearing") {
    const what = l.note ? ` (${l.note})` : scopeLabel(l.scope);
    return `«${l.key}» → «${l.value}»${what}`;
  }
  return l.value;
}

function scopeLabel(scope: string): string {
  if (!scope) return "";
  const key = scope.split(".").pop() ?? "";
  if (scope.startsWith("mcp__")) return key === "area" || key === "room" ? " (комната)" : " (устройство)";
  const tool = scope.split(".")[0];
  if (tool === "app_launch" || tool === "apps_find") return " (приложение)";
  if (tool === "open_target") return " (сайт или файл)";
  return "";
}

/** Lessons by kind in display order; empty kinds are left out. */
export function groupLessons(lessons: Lesson[]): { kind: LessonKind; items: Lesson[] }[] {
  return KIND_ORDER.map((kind) => ({
    kind,
    items: lessons
      .filter((l) => l.kind === kind)
      .sort((a, b) => b.hits - a.hits || b.updatedAt.localeCompare(a.updatedAt)),
  })).filter((g) => g.items.length > 0);
}

/** Sample lessons for the QA page without the native app. */
export const SAMPLE_LESSONS: Lesson[] = [
  { id: 1, kind: "preference", key: "", value: "Отвечать короче, без вопроса в конце", scope: "", note: "", source: "reflect", hits: 3, createdAt: "2026-11-20T10:00:00Z", updatedAt: "2026-11-23T10:00:00Z" },
  { id: 2, kind: "preference", key: "", value: "Свет вечером — тёплый, 30–40 %", scope: "", note: "", source: "model", hits: 1, createdAt: "2026-11-21T20:00:00Z", updatedAt: "2026-11-21T20:00:00Z" },
  { id: 3, kind: "humor", key: "", value: "Смеётся над сухой иронией про технику, не любит шутки про возраст", scope: "", note: "", source: "reflect", hits: 0, createdAt: "2026-11-22T10:00:00Z", updatedAt: "2026-11-22T10:00:00Z" },
  { id: 4, kind: "alias", key: "ночник", value: "Bedside lamp", scope: "mcp__home-assistant__*.name", note: "", source: "auto", hits: 7, createdAt: "2026-11-20T23:00:00Z", updatedAt: "2026-11-23T23:10:00Z" },
  { id: 5, kind: "alias", key: "зал", value: "Гостиная", scope: "mcp__home-assistant__*.area", note: "", source: "auto", hits: 4, createdAt: "2026-11-21T19:00:00Z", updatedAt: "2026-11-23T19:00:00Z" },
  { id: 6, kind: "hearing", key: "кровайте", value: "закрой все", scope: "", note: "", source: "model", hits: 0, createdAt: "2026-11-22T12:00:00Z", updatedAt: "2026-11-22T12:00:00Z" },
];
