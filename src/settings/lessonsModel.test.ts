import { describe, expect, it } from "vitest";
import { groupLessons, kindTitle, lessonText, SAMPLE_LESSONS, SOURCE_LABEL, type Lesson } from "./lessonsModel";

describe("lessons", () => {
  it("groups by kind in a fixed order, most used first", () => {
    const groups = groupLessons(SAMPLE_LESSONS);
    expect(groups.map((g) => g.kind)).toEqual(["preference", "humor", "alias", "hearing"]);
    expect(groups[2]?.items.map((l) => l.key)).toEqual(["ночник", "зал"]);
    expect(groupLessons([])).toEqual([]);
  });

  it("reads aliases and mishearings as «word» → «target» with what the target is", () => {
    const byId = (id: number) => SAMPLE_LESSONS.find((l) => l.id === id) as Lesson;
    expect(lessonText(byId(4))).toBe("«ночник» → «Bedside lamp» (устройство)");
    expect(lessonText(byId(5))).toBe("«зал» → «Гостиная» (комната)");
    expect(lessonText(byId(6))).toBe("«кровайте» → «закрой все»");
    expect(lessonText({ ...byId(4), note: "свет в спальне" })).toBe("«ночник» → «Bedside lamp» (свет в спальне)");
    expect(lessonText({ ...byId(4), scope: "app_launch.query", key: "телега", value: "Telegram" })).toBe("«телега» → «Telegram» (приложение)");
    expect(lessonText(byId(1))).toBe("Отвечать короче, без вопроса в конце");
  });

  it("has titles for both forms of address and labels for every source", () => {
    expect(kindTitle("alias", true)).toBe("Ваши слова");
    expect(kindTitle("alias", false)).toBe("Твои слова");
    for (const l of SAMPLE_LESSONS) expect(SOURCE_LABEL[l.source]).toBeTruthy();
  });
});
