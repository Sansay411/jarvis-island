// While a shelf file is being dragged out of the island, any drag events the
// island itself receives come from that same drag: they must never open the
// drop view or send the file anywhere. Also the small pure rules for where a
// drop goes and when the shelf row keeps the wheel for itself.

let outgoing: Set<string> | null = null;

export function beginDragOut(paths: string[]) {
  outgoing = new Set(paths);
}

export function endDragOut() {
  outgoing = null;
}

/** A drag event during our own drag-out (paths empty on over/leave). */
export function isOwnDrag(paths: string[]): boolean {
  if (!outgoing) return false;
  return paths.length === 0 || paths.every((p) => outgoing!.has(p));
}

/** Exact drop hit-test wins; the last `over` flag only when no point came. */
export function dropGoesToSend(exactHit: boolean | null, lastOver: boolean): boolean {
  return exactHit ?? lastOver;
}

export function rowCapturesWheel(scrollWidth: number, clientWidth: number): boolean {
  return scrollWidth > clientWidth;
}
