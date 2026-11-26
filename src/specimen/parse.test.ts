import { describe, expect, it } from "vitest";
import css from "../styles/tokens.css?raw";
import icon from "../ui/Icon.tsx?raw";
import { groupTokens, parseIconNames, parseTokens, resolveToken, tokenKind } from "./parse";

describe("parseTokens", () => {
  const tokens = parseTokens(css);

  it("reads every custom property of tokens.css", () => {
    const declared = css.match(/^\s*--[\w-]+\s*:/gm)!.length;
    expect(tokens).toHaveLength(declared);
    expect(tokens.find((t) => t.name === "--amber")).toMatchObject({ value: "#ffb23f", group: "Presence" });
  });

  it("groups by the comment above and keeps multi-line notes", () => {
    const groups = groupTokens(tokens).map((g) => g.group);
    expect(groups.slice(0, 3)).toEqual(["Surfaces", "Lines", "Text"]);
    const bone = tokens.find((t) => t.name === "--bone")!;
    expect(bone.group).toBe("The island is monochrome");
    expect(bone.note).toContain("bone for the user's own primary actions");
  });

  it("resolves var() chains and classifies kinds", () => {
    const cobalt = tokens.find((t) => t.name === "--cobalt")!;
    expect(resolveToken(cobalt.value, tokens)).toBe("#ecebe6");
    const kind = (n: string) => tokenKind(tokens.find((t) => t.name === n)!, tokens);
    expect(kind("--cobalt")).toBe("color");
    expect(kind("--line")).toBe("color");
    expect(kind("--font-ui")).toBe("font");
    expect(kind("--fs-13")).toBe("size");
    expect(kind("--r-pill")).toBe("size");
    expect(kind("--dur-fast")).toBe("duration");
    expect(kind("--ease-out")).toBe("easing");
    expect(kind("--skin-rgb")).toBe("other");
  });
});

describe("parseIconNames", () => {
  it("lists the icon table in source order", () => {
    const names = parseIconNames(icon);
    expect(names.slice(0, 3)).toEqual(["play", "pause", "next"]);
    expect(names).toContain("calendar");
    expect(new Set(names).size).toBe(names.length);
  });
});
