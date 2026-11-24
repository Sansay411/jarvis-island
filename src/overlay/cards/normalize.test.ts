import { describe, expect, it } from "vitest";
import { FALLBACK_TITLE, normalizeCard, parseNumber } from "./normalize";
import { CARD_LIMITS } from "./types";

describe("normalizeCard", () => {
  it("returns null for an invalid root or no usable blocks; a missing title falls back", () => {
    expect(normalizeCard(null)).toBeNull();
    expect(normalizeCard("text")).toBeNull();
    expect(normalizeCard({ title: "   ", blocks: [{ kind: "text", text: "ok" }] })?.title).toBe(FALLBACK_TITLE);
    expect(normalizeCard({ title: "Нет блоков", blocks: [] })).toBeNull();
    expect(normalizeCard({ title: "Только мусор", blocks: [{ kind: "unknown" }, 42] })).toBeNull();
  });

  it("coerces numeric strings, alternative keys, and clamps sizes", () => {
    const raw = {
      title: "  " + "А".repeat(CARD_LIMITS.maxTitle + 30),
      subtitle: "  Подзаголовок  ",
      blocks: [
        { kind: "text", content: "**Жирный** и *курсив*" },
        { kind: "steps", steps: ["Первый шаг", { label: "Второй", detail: "Пояснение" }] },
        {
          kind: "table",
          headers: ["Колонка 1", "Колонка 2"],
          rows: Array.from({ length: 20 }, (_, i) => [`Строка ${i}`, i, "лишнее"]),
          highlight_col: "1",
        },
        {
          kind: "bars",
          unit: "мс",
          items: [
            { label: "Кэш", value: "12,5", highlight: "true" },
            { label: "Невалидный", value: "abc" },
            ...Array.from({ length: 15 }, (_, i) => ({ label: `Элемент ${i}`, value: i + 1 })),
          ],
        },
        {
          kind: "line",
          points: [
            { x: "Пн", y: "10" },
            { x: "Вт", y: 20 },
            { x: "Ср", y: "15.5" },
          ],
        },
        {
          kind: "flow",
          direction: "LR",
          nodes: [
            { id: "a", label: "Старт", tone: "accent" },
            { id: "b", label: "Финиш" },
            { id: "a", label: "Дубль" },
          ],
          edges: [
            { source: "a", target: "b", label: "далее" },
            { source: "a", target: "missing" },
            { source: "a", target: "b", label: "далее" },
          ],
        },
        {
          kind: "compare",
          columns: [
            { title: "Вариант 1", pros: ["Быстро"], cons: ["Дороже"] },
            { title: "Вариант 2", items: ["Просто", { text: "Дешевле", kind: "+" }] },
          ],
        },
        {
          kind: "quiz",
          question: "2 + 2?",
          options: ["3", "4", "5"],
          correct_index: "99",
          explanation: "Правильно 4",
        },
        // 9th block should be dropped because maxBlocks is 8
        { kind: "kv", items: [{ key: "лишний", value: "блок" }] },
      ],
    };

    const card = normalizeCard(raw);
    expect(card).not.toBeNull();
    expect(Array.from(card!.title).length).toBe(CARD_LIMITS.maxTitle);
    expect(card!.title.endsWith("…")).toBe(true);
    expect(card!.subtitle).toBe("Подзаголовок");
    expect(card!.blocks).toHaveLength(CARD_LIMITS.maxBlocks);

    const table = card!.blocks[2]!;
    expect(table.kind).toBe("table");
    if (table.kind === "table") {
      expect(table.rows).toHaveLength(CARD_LIMITS.maxTableRows);
      expect(table.rows[0]).toEqual(["Строка 0", "0"]);
      expect(table.highlightCol).toBe(1);
    }

    const bars = card!.blocks[3]!;
    expect(bars.kind).toBe("bars");
    if (bars.kind === "bars") {
      expect(bars.items).toHaveLength(CARD_LIMITS.maxBars);
      expect(bars.items[0]).toEqual({ label: "Кэш", value: 12.5, highlight: true });
    }

    const line = card!.blocks[4]!;
    expect(line.kind).toBe("line");
    if (line.kind === "line") {
      expect(line.xLabels).toEqual(["Пн", "Вт", "Ср"]);
      expect(line.series).toEqual([{ values: [10, 20, 15.5] }]);
    }

    const flow = card!.blocks[5]!;
    expect(flow.kind).toBe("flow");
    if (flow.kind === "flow") {
      expect(flow.direction).toBe("lr");
      expect(flow.nodes).toHaveLength(2);
      expect(flow.edges).toEqual([{ from: "a", to: "b", label: "далее" }]);
    }

    const compare = card!.blocks[6]!;
    expect(compare.kind).toBe("compare");
    if (compare.kind === "compare") {
      expect(compare.columns[0]!.items).toEqual([
        { text: "Быстро", kind: "pro" },
        { text: "Дороже", kind: "con" },
      ]);
      expect(compare.columns[1]!.items).toEqual([
        { text: "Просто" },
        { text: "Дешевле", kind: "pro" },
      ]);
    }

    const quiz = card!.blocks[7]!;
    expect(quiz.kind).toBe("quiz");
    if (quiz.kind === "quiz") {
      expect(quiz.correct).toBe(2);
    }
  });

  it("normalizes timeline, math, definition, callout, and kv blocks", () => {
    const card = normalizeCard({
      title: "Проверка остальных блоков",
      blocks: [
        { kind: "timeline", items: [{ date: 1991, label: "Релиз", note: "Первый" }] },
        { kind: "math", lines: ["x + 1 = 3", { expression: "x = 2", comment: "готово" }], answer: "2" },
        { kind: "definition", term: "API", meaning: "Интерфейс программ", example: "REST" },
        { kind: "callout", tone: "weird", text: "Важная мысль" },
        { kind: "kv", items: [{ key: "Версия", value: 3.13 }] },
      ],
    });
    expect(card).not.toBeNull();
    expect(card!.blocks.map((b) => b.kind)).toEqual(["timeline", "math", "definition", "callout", "kv"]);
    const callout = card!.blocks[3]!;
    if (callout.kind === "callout") expect(callout.tone).toBe("idea");
    const kv = card!.blocks[4]!;
    if (kv.kind === "kv") expect(kv.items[0]).toEqual({ key: "Версия", value: "3.13" });
  });

  it("drops quizzes with fewer than 2 valid options and lines with fewer than 2 points", () => {
    const card = normalizeCard({
      title: "Границы",
      blocks: [
        { kind: "quiz", question: "Один?", options: ["Только один"], correct: 0 },
        { kind: "line", xLabels: ["A"], values: [1] },
        { kind: "text", text: "Остаётся только этот блок" },
      ],
    });
    expect(card).not.toBeNull();
    expect(card!.blocks).toHaveLength(1);
    expect(card!.blocks[0]!.kind).toBe("text");
  });

  it("reads numbers the way people write them", () => {
    expect(parseNumber("42,5")).toBe(42.5);
    expect(parseNumber("1 200 ₽")).toBe(1200);
    expect(parseNumber("$1,234.5")).toBe(1234.5);
    expect(parseNumber("1.234,5")).toBe(1234.5);
    expect(parseNumber("1,500")).toBe(1500);
    expect(parseNumber("0,125")).toBe(0.125);
    expect(parseNumber("−7 %")).toBe(-7);
    expect(parseNumber("около 15 мс")).toBe(15);
    expect(parseNumber("abc")).toBeNull();
    expect(parseNumber(Number.NaN)).toBeNull();
    expect(parseNumber(true)).toBeNull();
  });

  it("accepts stringified blocks, kind aliases and shapes without a kind", () => {
    const card = normalizeCard({
      title: "Небрежная карточка",
      blocks: JSON.stringify([
        "Просто строка становится текстом",
        { type: "bar_chart", data: { Январь: "10", Февраль: 12 } },
        { type: "warning", text: "Осторожно" },
        { type: "key-value", facts: { Порт: 443, Протокол: "HTTPS" } },
        { headers: ["", "A"], rows: [{ "": "x", a: 1 }] },
        { nodes: ["Запрос", "Ответ"] },
        { question: "2 + 2?", options: ["3", "4"], answer: "4" },
        { type: "chart", series: [{ name: "S", data: [{ x: "Пн", y: 1 }, { x: "Вт", y: 2 }] }] },
      ]),
    });
    expect(card).not.toBeNull();
    const kinds = card!.blocks.map((b) => b.kind);
    expect(kinds).toEqual(["text", "bars", "callout", "kv", "table", "flow", "quiz", "line"]);
    const [, bars, callout, kv, table, flow, quiz, line] = card!.blocks;
    expect(bars).toMatchObject({ items: [{ label: "Январь", value: 10 }, { label: "Февраль", value: 12 }] });
    expect(callout).toMatchObject({ tone: "warning" });
    expect(kv).toMatchObject({ items: [{ key: "Порт", value: "443" }, { key: "Протокол", value: "HTTPS" }] });
    expect(table).toMatchObject({ headers: ["", "A"], rows: [["x", "1"]] });
    // Nodes without arrows read as a chain.
    expect(flow).toMatchObject({ edges: [{ from: "Запрос", to: "Ответ" }] });
    expect(quiz).toMatchObject({ correct: 1 });
    expect(line).toMatchObject({ xLabels: ["Пн", "Вт"], series: [{ name: "S", values: [1, 2] }] });
  });

  it("resolves quiz answers by index, text, letter or flag and drops unknown ones", () => {
    const quiz = (extra: Record<string, unknown>, options: unknown[] = ["3", "4", "5"]) =>
      normalizeCard({ title: "Q", blocks: [{ kind: "quiz", question: "2 + 2?", options, ...extra }] })?.blocks[0];
    expect(quiz({ correct: 1 })).toMatchObject({ correct: 1 });
    expect(quiz({ correct: "4" })).toMatchObject({ correct: 1 });
    expect(quiz({ answer: "Б" })).toMatchObject({ correct: 1 });
    expect(quiz({ answer: "b)" })).toMatchObject({ correct: 1 });
    expect(quiz({}, ["3", { text: "4", correct: true }, "5"])).toMatchObject({ correct: 1 });
    expect(quiz({ answer: "не знаю" })).toBeUndefined();
    expect(quiz({})).toBeUndefined();
  });

  it("reads flow edges written as arrows, tuples and labels", () => {
    const card = normalizeCard({
      title: "Схема",
      blocks: [
        {
          kind: "flow",
          direction: "horizontal",
          nodes: [{ id: "a", label: "Клиент", tone: "highlight" }, { id: "b", label: "Сервер" }, "База"],
          edges: ["Клиент -> b: HTTP", ["b", "База"], { source: "a", target: "nowhere" }],
        },
        { kind: "flow", edges: ["Идея → Прототип", "Прототип ==> Релиз"] },
      ],
    });
    expect(card!.blocks[0]).toEqual({
      kind: "flow",
      direction: "lr",
      nodes: [
        { id: "a", label: "Клиент", tone: "accent" },
        { id: "b", label: "Сервер" },
        { id: "База", label: "База" },
      ],
      edges: [
        { from: "a", to: "b", label: "HTTP" },
        { from: "b", to: "База" },
      ],
    });
    // No nodes listed: they come from the arrows.
    expect(card!.blocks[1]).toMatchObject({ nodes: [{ id: "Идея" }, { id: "Прототип" }, { id: "Релиз" }] });
  });

  it("reads compare prefixes, joins list values and strips control characters", () => {
    const card = normalizeCard({
      title: "Сравнение\u0007",
      blocks: [
        { kind: "compare", columns: [{ title: "A", items: ["+ Быстро", "− Дорого", "Просто"], recommended: "yes" }] },
        { kind: "definition", term: "Кэш", meaning: "Быстрая память", examples: ["L1", "L2"] },
      ],
    });
    expect(card!.title).toBe("Сравнение");
    expect(card!.blocks[0]).toEqual({
      kind: "compare",
      columns: [
        {
          title: "A",
          accent: true,
          items: [{ text: "Быстро", kind: "pro" }, { text: "Дорого", kind: "con" }, { text: "Просто" }],
        },
      ],
    });
    expect(card!.blocks[1]).toMatchObject({ example: "L1; L2" });
  });

  it("is idempotent, so the overlay may normalize an already clean card again", () => {
    const once = normalizeCard({
      title: "Всё сразу",
      subtitle: "s",
      blocks: [
        { kind: "table", headers: ["", "B"], rows: [["a", 1]], highlightCol: 1 },
        { kind: "flow", nodes: [{ id: "x", label: "X", tone: "muted", note: "n" }, "Y"], edges: [] },
        { kind: "callout", tone: "tip", title: "t", text: "c" },
        { kind: "quiz", question: "?", options: ["a", "b"], correct: 1, explanation: "e" },
        { kind: "line", values: [1, 2, 3] },
        { kind: "compare", columns: [{ title: "A", pros: ["p"], cons: ["c"] }] },
        { kind: "math", lines: ["x = 1"], result: "1" },
        { kind: "kv", items: [{ key: "k", value: "v" }] },
      ],
    });
    expect(once).not.toBeNull();
    expect(normalizeCard(once)).toEqual(once);
  });
});
