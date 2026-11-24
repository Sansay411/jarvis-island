import { describe, expect, it } from "vitest";
import { DEFAULT_SKIN, SKINS, normalizeSkin, skinCssVars, skinMeta } from "./types";

describe("skins", () => {
  it("defaults to amber and exposes all five skins with Russian labels", () => {
    expect(DEFAULT_SKIN).toBe("amber");
    expect(SKINS.map((s) => s.id)).toEqual(["amber", "orb", "ice", "pixel", "minimal"]);
    expect(SKINS.map((s) => s.label)).toEqual(["Янтарь", "Сфера", "Лёд", "Пиксель", "Минимал"]);
    expect(SKINS.every((s) => s.desc.trim().length > 0)).toBe(true);
  });

  it("normalizes unknown, empty or missing skin ids to amber", () => {
    expect(normalizeSkin(undefined)).toBe("amber");
    expect(normalizeSkin(null)).toBe("amber");
    expect(normalizeSkin("")).toBe("amber");
    expect(normalizeSkin("neon-purple")).toBe("amber");
    for (const s of SKINS) {
      expect(normalizeSkin(s.id)).toBe(s.id);
    }
  });

  it("produces matching CSS variables and preserves amber defaults", () => {
    const amberVars = skinCssVars("amber") as Record<string, string>;
    expect(amberVars["--skin-accent"]).toBe("#ffb23f");
    expect(amberVars["--skin-rgb"]).toBe("255, 178, 63");

    for (const s of SKINS) {
      const vars = skinCssVars(s.id) as Record<string, string>;
      const meta = skinMeta(s.id);
      expect(vars["--skin-accent"]).toBe(meta.accent);
      expect(vars["--skin-accent-hi"]).toBe(meta.accentHi);
      expect(vars["--skin-accent-lo"]).toBe(meta.accentLo);
      expect(vars["--skin-glow"]).toBe(meta.glow);
      expect(vars["--skin-rgb"]).toBe(meta.rgb);
    }
  });
});
