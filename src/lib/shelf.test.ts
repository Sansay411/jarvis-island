import { describe, expect, it } from "vitest";
import { mergeShelf } from "./shelf";
import type { ShelfItem } from "./types";

const item = (id: string, kind = "file"): ShelfItem => ({
  id,
  kind,
  label: id,
  value: id,
  subtitle: "",
  sizeBytes: null,
  exists: true,
  pinned: false,
  createdAt: "",
});

describe("mergeShelf", () => {
  it("keeps what was on the shelf when files are added", () => {
    const prev = [item("text-1", "text")];
    const next = mergeShelf(prev, [item("img-1")]);
    expect(next.map((i) => i.id)).toEqual(["img-1", "text-1"]);
  });

  it("moves a re-added file to the front without duplicating it", () => {
    const prev = [item("a"), item("b"), item("c")];
    const next = mergeShelf(prev, [item("c")]);
    expect(next.map((i) => i.id)).toEqual(["c", "a", "b"]);
  });
});
