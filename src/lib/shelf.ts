// The shelf list after files were added: the added items go first, and
// everything that was already there stays (Rust returns only the added ones).

import type { ShelfItem } from "./types";

export function mergeShelf(prev: ShelfItem[], added: ShelfItem[]): ShelfItem[] {
  const ids = new Set(added.map((i) => i.id));
  return [...added, ...prev.filter((i) => !ids.has(i.id))];
}
