// Calendar maths in local wall time. Events travel as strings —
// `YYYY-MM-DDTHH:MM` (timed) or `YYYY-MM-DD` (all-day), ends exclusive —
// exactly as the Rust planner stores them.

export type ViewKind = "day" | "4day" | "week" | "month";

export const pad = (n: number) => String(n).padStart(2, "0");

export function isoDate(d: Date): string {
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

export function isoDateTime(d: Date): string {
  return `${isoDate(d)}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** `YYYY-MM-DD` or `YYYY-MM-DDTHH:MM[:SS]` as a local Date. */
export function parseLocal(s: string): Date {
  const [date, time = "00:00"] = s.split("T");
  const [y, m, d] = (date ?? "").split("-").map(Number);
  const [hh, mm] = time.split(":").map(Number);
  return new Date(y ?? 1970, (m ?? 1) - 1, d ?? 1, hh ?? 0, mm ?? 0, 0, 0);
}

export function addDays(d: Date, n: number): Date {
  const r = new Date(d);
  r.setDate(r.getDate() + n);
  return r;
}

export function addMinutes(d: Date, n: number): Date {
  return new Date(d.getTime() + n * 60_000);
}

export function startOfDay(d: Date): Date {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate());
}

/** Monday-first weeks (Russian calendars). */
export function startOfWeek(d: Date): Date {
  const s = startOfDay(d);
  const dow = (s.getDay() + 6) % 7;
  return addDays(s, -dow);
}

export function startOfMonth(d: Date): Date {
  return new Date(d.getFullYear(), d.getMonth(), 1);
}

export function sameDay(a: Date, b: Date): boolean {
  return a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate();
}

export function minutesOfDay(d: Date): number {
  return d.getHours() * 60 + d.getMinutes();
}

/** Whole days between two dates (DST-safe). */
export function dayDiff(a: Date, b: Date): number {
  const ua = Date.UTC(a.getFullYear(), a.getMonth(), a.getDate());
  const ub = Date.UTC(b.getFullYear(), b.getMonth(), b.getDate());
  return Math.round((ub - ua) / 86_400_000);
}

export function snap(minutes: number, step = 15): number {
  return Math.round(minutes / step) * step;
}

/** The first visible day and how many columns a view shows. */
export function viewRange(view: ViewKind, anchor: Date): { start: Date; days: number } {
  switch (view) {
    case "day":
      return { start: startOfDay(anchor), days: 1 };
    case "4day":
      return { start: startOfDay(anchor), days: 4 };
    case "week":
      return { start: startOfWeek(anchor), days: 7 };
    case "month": {
      const first = startOfWeek(startOfMonth(anchor));
      return { start: first, days: 42 };
    }
  }
}

/** The anchor after «‹» / «›». */
export function step(view: ViewKind, anchor: Date, dir: 1 | -1): Date {
  switch (view) {
    case "day":
      return addDays(anchor, dir);
    case "4day":
      return addDays(anchor, 4 * dir);
    case "week":
      return addDays(anchor, 7 * dir);
    case "month":
      return new Date(anchor.getFullYear(), anchor.getMonth() + dir, 1);
  }
}

export const WEEKDAYS_SHORT = ["ПН", "ВТ", "СР", "ЧТ", "ПТ", "СБ", "ВС"];
export const WEEKDAYS_MINI = ["П", "В", "С", "Ч", "П", "С", "В"];
export const WEEKDAYS_LONG = ["понедельник", "вторник", "среда", "четверг", "пятница", "суббота", "воскресенье"];
export const MONTHS = ["январь", "февраль", "март", "апрель", "май", "июнь", "июль", "август", "сентябрь", "октябрь", "ноябрь", "декабрь"];
export const MONTHS_GEN = ["января", "февраля", "марта", "апреля", "мая", "июня", "июля", "августа", "сентября", "октября", "ноября", "декабря"];
export const MONTHS_SHORT = ["янв.", "февр.", "мар.", "апр.", "мая", "июн.", "июл.", "авг.", "сент.", "окт.", "нояб.", "дек."];

export const weekdayIndex = (d: Date) => (d.getDay() + 6) % 7;

const cap = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);

/** «Ноябрь 2026», «Ноябрь – декабрь 2026», «Декабрь 2026 – январь 2027». */
export function periodTitle(view: ViewKind, anchor: Date): string {
  if (view === "month") return `${cap(MONTHS[anchor.getMonth()]!)} ${anchor.getFullYear()}`;
  if (view === "day") return `${anchor.getDate()} ${MONTHS_GEN[anchor.getMonth()]} ${anchor.getFullYear()}`;
  const { start, days } = viewRange(view, anchor);
  const end = addDays(start, days - 1);
  if (start.getMonth() === end.getMonth()) return `${cap(MONTHS[start.getMonth()]!)} ${start.getFullYear()}`;
  if (start.getFullYear() === end.getFullYear()) {
    return `${cap(MONTHS[start.getMonth()]!)} – ${MONTHS[end.getMonth()]} ${end.getFullYear()}`;
  }
  return `${cap(MONTHS[start.getMonth()]!)} ${start.getFullYear()} – ${MONTHS[end.getMonth()]} ${end.getFullYear()}`;
}

export function timeLabel(minutes: number): string {
  const m = ((minutes % 1440) + 1440) % 1440;
  return `${pad(Math.floor(m / 60))}:${pad(m % 60)}`;
}

export function hm(d: Date): string {
  return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** «Понедельник, 23 ноября» */
export function dayLabel(d: Date): string {
  return `${cap(WEEKDAYS_LONG[weekdayIndex(d)]!)}, ${d.getDate()} ${MONTHS_GEN[d.getMonth()]}`;
}

/** «Понедельник, 23 ноября · 15:00 – 16:00», all-day and multi-day aware. */
export function spanLabel(start: string, end: string, allDay: boolean): string {
  const s = parseLocal(start);
  const e = parseLocal(end);
  if (allDay) {
    const last = addDays(e, -1);
    return sameDay(s, last) ? dayLabel(s) : `${dayLabel(s)} – ${dayLabel(last)}`;
  }
  if (sameDay(s, e) || (minutesOfDay(e) === 0 && dayDiff(s, e) === 1)) return `${dayLabel(s)} · ${hm(s)} – ${hm(e)}`;
  return `${dayLabel(s)}, ${hm(s)} – ${dayLabel(e)}, ${hm(e)}`;
}

/** «GMT+05» like Google's corner label. */
export function gmtLabel(d = new Date()): string {
  const off = -d.getTimezoneOffset();
  const sign = off >= 0 ? "+" : "-";
  const a = Math.abs(off);
  const h = pad(Math.floor(a / 60));
  return a % 60 === 0 ? `GMT${sign}${h}` : `GMT${sign}${h}:${pad(a % 60)}`;
}

/** Minutes → «1 ч 30 мин». */
export function durationLabel(minutes: number): string {
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  if (h === 0) return `${m} мин`;
  return m === 0 ? `${h} ч` : `${h} ч ${m} мин`;
}
