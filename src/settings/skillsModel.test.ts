import { describe, expect, it } from "vitest";
import {
  filterSkills,
  formatBytesKb,
  formatLastUsed,
  formatUses,
  looksLikeSkillUrl,
  SAMPLE_SKILLS,
  utf8ByteLength,
} from "./skillsModel";

describe("skillsModel", () => {
  it("formats Russian usage counts with accurate pluralization", () => {
    expect(formatUses(0)).toBe("ещё не применялся");
    expect(formatUses(1)).toBe("1 раз");
    expect(formatUses(2)).toBe("2 раза");
    expect(formatUses(4)).toBe("4 раза");
    expect(formatUses(5)).toBe("5 раз");
    expect(formatUses(11)).toBe("11 раз");
    expect(formatUses(14)).toBe("14 раз");
    expect(formatUses(21)).toBe("21 раз");
    expect(formatUses(23)).toBe("23 раза");
  });

  it("measures UTF-8 byte lengths for Cyrillic text and formats sizes", () => {
    expect(utf8ByteLength("abc")).toBe(3);
    expect(utf8ByteLength("привет")).toBe(12);
    expect(formatBytesKb(512)).toBe("512 Б");
    expect(formatBytesKb(2048)).toBe("2,0 КБ");
  });

  it("validates public HTTP(S) skill URLs", () => {
    expect(looksLikeSkillUrl("https://github.com/org/repo/blob/main/SKILL.md")).toBe(true);
    expect(looksLikeSkillUrl("http://example.com/SKILL.md")).toBe(true);
    expect(looksLikeSkillUrl("file:///C:/SKILL.md")).toBe(false);
    expect(looksLikeSkillUrl("not a url")).toBe(false);
    expect(looksLikeSkillUrl("   ")).toBe(false);
  });

  it("filters skills ignoring case and ё/е", () => {
    const res = filterSkills(SAMPLE_SKILLS, "отчета");
    expect(res.map((s) => s.slug)).toEqual(["vygruzka-otcheta-za-nedelyu"]);
    expect(filterSkills(SAMPLE_SKILLS, "").length).toBe(SAMPLE_SKILLS.length);
  });

  it("formats lastUsed cleanly and rejects invalid dates", () => {
    const now = new Date("2026-11-24T15:00:00Z");
    expect(formatLastUsed(null, now)).toBeNull();
    expect(formatLastUsed("not-a-date", now)).toBeNull();
    expect(formatLastUsed("2026-11-24T12:00:00Z", now)).toContain("сегодня");
  });
});
