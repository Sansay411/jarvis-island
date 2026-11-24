import { describe, expect, it } from "vitest";
import { itemLifetimeMs, liveItems, mergeItems, wrapItem } from "./items";

describe("overlay items", () => {
  it("matches the backend lifetimes", () => {
    expect(itemLifetimeMs({ kind: "ring", x: 0, y: 0, w: 10, h: 10 })).toBe(8000);
    expect(itemLifetimeMs({ kind: "arrow", to: { x: 1, y: 1 }, seconds: 2.5 })).toBe(2500);
    expect(itemLifetimeMs({ kind: "ring", x: 0, y: 0, w: 10, h: 10, persistent: true })).toBeNull();
    expect(itemLifetimeMs({ kind: "caption", text: "a", seconds: 0 })).toBeNull();
    expect(itemLifetimeMs({ kind: "card", id: "c", spec: { title: "Карточка", blocks: [] } })).toBeNull();
  });

  it("replaces, updates by id and appends", () => {
    const a = wrapItem({ kind: "ring", id: "r", x: 0, y: 0, w: 1, h: 1 }, 0);
    const b = wrapItem({ kind: "caption", text: "b" }, 0);
    const a2 = wrapItem({ kind: "ring", id: "r", x: 5, y: 5, w: 1, h: 1 }, 0);
    const c = wrapItem({ kind: "card", id: "c", spec: { title: "Карточка", blocks: [] } }, 0);

    expect(mergeItems([a, b], [c], true)).toEqual([c]);
    const merged = mergeItems([a, b], [a2, c], false);
    expect(merged.map((m) => m.key)).toEqual(["r", b.key, "c"]);
    expect(merged[0]?.item).toBe(a2.item);
  });

  it("drops expired items only", () => {
    const timed = wrapItem({ kind: "ring", x: 0, y: 0, w: 1, h: 1, seconds: 1 }, 1000);
    const kept = wrapItem({ kind: "card", id: "c", spec: { title: "Карточка", blocks: [] } }, 1000);
    expect(liveItems([timed, kept], 1500)).toHaveLength(2);
    expect(liveItems([timed, kept], 2000)).toEqual([kept]);
  });
});
