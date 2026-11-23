// Where events sit on the grid. Timed events of one day share the column
// the way Google Calendar does it: clusters of overlapping events split the
// width into columns, and an event widens over free columns to its right.
// All-day and multi-day events stack into rows across the visible days.

import { addDays, dayDiff, parseLocal, startOfDay } from "./dates";

export interface Span {
  start: string;
  end: string;
  allDay: boolean;
}

export interface Placed<T> {
  item: T;
  /** Minutes from the day's midnight. */
  top: number;
  /** Minutes of height (the visible part on this day). */
  height: number;
  /** 0..1 */
  left: number;
  width: number;
  /** Continues from the previous day / onto the next. */
  fromBefore: boolean;
  intoNext: boolean;
  /** Paint order: later starts sit on top of the events they overlap. */
  z: number;
}

/** Shortest drawn event, so a 5-minute block still has room for its title. */
const MIN_VISUAL = 20;
/** A later start by this much overlaps instead of squeezing beside. */
const CASCADE_GAP = 30 * 60_000;

/** Is it drawn in the all-day row (all-day, or timed and 24 h or longer)? */
export function isBanner(e: Span): boolean {
  if (e.allDay) return true;
  return parseLocal(e.end).getTime() - parseLocal(e.start).getTime() >= 24 * 3600_000;
}

export function layoutDay<T extends Span>(items: T[], day: Date): Placed<T>[] {
  const d0 = startOfDay(day).getTime();
  const d1 = addDays(startOfDay(day), 1).getTime();
  const rows = items
    .filter((e) => !isBanner(e))
    .map((item) => {
      const s = parseLocal(item.start).getTime();
      const e = parseLocal(item.end).getTime();
      const vs = Math.max(s, d0);
      const ve = Math.min(e, d1);
      return { item, s: vs, e: ve, fromBefore: s < d0, intoNext: e > d1 };
    })
    .filter((r) => r.e > r.s)
    .sort((a, b) => a.s - b.s || b.e - a.e);

  const out: Placed<T>[] = [];
  let cluster: typeof rows = [];
  let clusterEnd = -Infinity;
  const flush = () => {
    if (cluster.length === 0) return;
    // Greedy columns by visual end (tiny events still take MIN_VISUAL).
    const colEnds: number[] = [];
    const colOf = new Map<(typeof rows)[number], number>();
    for (const r of cluster) {
      const vEnd = Math.max(r.e, r.s + MIN_VISUAL * 60_000);
      let c = colEnds.findIndex((end) => end <= r.s);
      if (c === -1) {
        c = colEnds.length;
        colEnds.push(vEnd);
      } else colEnds[c] = vEnd;
      colOf.set(r, c);
    }
    const cols = colEnds.length;
    cluster.forEach((r, order) => {
      const c = colOf.get(r)!;
      const vEnd = Math.max(r.e, r.s + MIN_VISUAL * 60_000);
      const overlapsIn = (k: number) => cluster.filter((o) => colOf.get(o) === k && o.s < vEnd && Math.max(o.e, o.s + MIN_VISUAL * 60_000) > r.s);
      // Widen over columns to the right that are free during this event.
      let span = 1;
      for (let k = c + 1; k < cols; k++) {
        if (overlapsIn(k).length > 0) break;
        span++;
      }
      let width = span / cols;
      // Google's cascade: when everything to the right starts at least half
      // an hour later, this event runs on under them, so its title stays
      // readable; the later ones are painted on top.
      const right = c + span < cols ? overlapsIn(c + span) : [];
      if (right.length > 0 && right.every((o) => o.s - r.s >= CASCADE_GAP)) {
        width = Math.min(1 - c / cols, width * 1.7);
      }
      out.push({
        item: r.item,
        top: (r.s - d0) / 60_000,
        height: Math.max((r.e - r.s) / 60_000, 0),
        left: c / cols,
        width,
        fromBefore: r.fromBefore,
        intoNext: r.intoNext,
        z: order + 1,
      });
    });
    cluster = [];
  };
  for (const r of rows) {
    const vEnd = Math.max(r.e, r.s + MIN_VISUAL * 60_000);
    if (cluster.length > 0 && r.s >= clusterEnd) {
      flush();
      clusterEnd = -Infinity;
    }
    cluster.push(r);
    clusterEnd = Math.max(clusterEnd, vEnd);
  }
  flush();
  return out;
}

export interface Banner<T> {
  item: T;
  /** First and last visible column (inclusive). */
  from: number;
  to: number;
  row: number;
  /** Starts before / ends after the visible range. */
  clippedStart: boolean;
  clippedEnd: boolean;
}

/** Rows of all-day bars over `days` columns starting at `start`. */
export function layoutBanners<T extends Span>(items: T[], start: Date, days: number, onlyBanners = true): Banner<T>[] {
  const s0 = startOfDay(start);
  const placed: Banner<T>[] = [];
  const rows: Array<Array<[number, number]>> = [];
  const list = items
    .filter((e) => !onlyBanners || isBanner(e))
    .map((item) => {
      const s = parseLocal(item.start);
      let e = parseLocal(item.end);
      // Exclusive ends: an all-day event ending on the 24th covers up to the 23rd;
      // a timed one ending at midnight does not touch the next day.
      if (item.allDay || (e.getHours() === 0 && e.getMinutes() === 0)) e = addDays(e, -1);
      const from = dayDiff(s0, s);
      const to = dayDiff(s0, e);
      return { item, from, to: Math.max(from, to) };
    })
    .filter((x) => x.to >= 0 && x.from < days)
    .sort((a, b) => a.from - b.from || b.to - b.from - (a.to - a.from));
  for (const x of list) {
    const from = Math.max(0, x.from);
    const to = Math.min(days - 1, x.to);
    let row = rows.findIndex((r) => r.every(([a, b]) => to < a || from > b));
    if (row === -1) {
      row = rows.length;
      rows.push([]);
    }
    rows[row]!.push([from, to]);
    placed.push({ item: x.item, from, to, row, clippedStart: x.from < 0, clippedEnd: x.to > days - 1 });
  }
  return placed;
}
