import { describe, expect, it } from "vitest";
import { FLOW_MAX_WIDTH, layoutFlow, type FlowNodeBox } from "./flowLayout";

function overlaps(a: FlowNodeBox, b: FlowNodeBox): boolean {
  return a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y;
}

function assertNoOverlaps(nodes: FlowNodeBox[]) {
  for (let i = 0; i < nodes.length; i++) {
    for (let j = i + 1; j < nodes.length; j++) {
      expect(overlaps(nodes[i]!, nodes[j]!), `nodes ${nodes[i]!.id} and ${nodes[j]!.id} overlap`).toBe(false);
    }
  }
}

describe("layoutFlow", () => {
  it("lays out a left-to-right chain with strictly increasing x coordinates", () => {
    const layout = layoutFlow({
      kind: "flow",
      direction: "lr",
      nodes: [
        { id: "a", label: "Запрос" },
        { id: "b", label: "Обработка" },
        { id: "c", label: "Ответ" },
      ],
      edges: [
        { from: "a", to: "b", label: "HTTP" },
        { from: "b", to: "c" },
      ],
    });

    expect(layout.direction).toBe("lr");
    expect(layout.nodes).toHaveLength(3);
    expect(layout.edges).toHaveLength(2);
    expect(layout.nodes[0]!.x).toBeLessThan(layout.nodes[1]!.x);
    expect(layout.nodes[1]!.x).toBeLessThan(layout.nodes[2]!.x);
    expect(layout.edges[0]!.labelPos).toBeDefined();
    expect(layout.edges[0]!.backEdge).toBe(false);
    assertNoOverlaps(layout.nodes);
  });

  it("lays out a branching diamond top-down with zero overlaps", () => {
    const layout = layoutFlow({
      kind: "flow",
      direction: "td",
      nodes: [
        { id: "start", label: "Вход" },
        { id: "left", label: "Кэш", note: "быстро" },
        { id: "right", label: "База", note: "диск" },
        { id: "end", label: "Результат", tone: "accent" },
      ],
      edges: [
        { from: "start", to: "left", label: "hit" },
        { from: "start", to: "right", label: "miss" },
        { from: "left", to: "end" },
        { from: "right", to: "end" },
      ],
    });

    expect(layout.direction).toBe("td");
    const byId = new Map(layout.nodes.map((n) => [n.id, n]));
    expect(byId.get("start")!.layer).toBe(0);
    expect(byId.get("left")!.layer).toBe(1);
    expect(byId.get("right")!.layer).toBe(1);
    expect(byId.get("end")!.layer).toBe(2);
    expect(byId.get("start")!.y).toBeLessThan(byId.get("left")!.y);
    expect(byId.get("left")!.y).toBeLessThan(byId.get("end")!.y);
    assertNoOverlaps(layout.nodes);
  });

  it("breaks cycles gracefully without infinite loops and marks back-edges", () => {
    const layout = layoutFlow({
      kind: "flow",
      direction: "lr",
      nodes: [
        { id: "a", label: "Клиент" },
        { id: "b", label: "Сервер" },
        { id: "c", label: "Кэш" },
      ],
      edges: [
        { from: "a", to: "b" },
        { from: "b", to: "c" },
        { from: "c", to: "a", label: "повтор" },
        { from: "b", to: "b", label: "retry" },
      ],
    });

    expect(layout.nodes).toHaveLength(3);
    expect(layout.edges).toHaveLength(4);
    expect(layout.edges.filter((e) => e.backEdge)).toHaveLength(2);
    expect(layout.width).toBeGreaterThan(0);
    expect(layout.height).toBeGreaterThan(0);
    assertNoOverlaps(layout.nodes);
  });

  it("auto-selects td for deep graphs and lr for shallow ones", () => {
    const shallow = layoutFlow({
      kind: "flow",
      nodes: [
        { id: "1", label: "A" },
        { id: "2", label: "B" },
      ],
      edges: [{ from: "1", to: "2" }],
    });
    expect(shallow.direction).toBe("lr");

    const deep = layoutFlow({
      kind: "flow",
      nodes: [
        { id: "1", label: "A" },
        { id: "2", label: "B" },
        { id: "3", label: "C" },
        { id: "4", label: "D" },
      ],
      edges: [
        { from: "1", to: "2" },
        { from: "2", to: "3" },
        { from: "3", to: "4" },
      ],
    });
    expect(deep.direction).toBe("td");
  });

  it("handles empty node lists safely", () => {
    const empty = layoutFlow({ kind: "flow", nodes: [], edges: [] });
    expect(empty.nodes).toEqual([]);
    expect(empty.edges).toEqual([]);
    expect(empty.width).toBe(0);
  });

  it("turns a requested left-to-right chain top-down when it would not fit the card", () => {
    const ids = ["a", "b", "c", "d", "e", "f"];
    const block = {
      kind: "flow" as const,
      direction: "lr" as const,
      nodes: ids.map((id) => ({ id, label: `Этап ${id.toUpperCase()}` })),
      edges: ids.slice(1).map((id, i) => ({ from: ids[i]!, to: id })),
    };
    const fitted = layoutFlow(block);
    expect(fitted.direction).toBe("td");
    expect(fitted.width).toBeLessThanOrEqual(FLOW_MAX_WIDTH);
    assertNoOverlaps(fitted.nodes);
    // With room enough the requested direction stays.
    expect(layoutFlow(block, 2000).direction).toBe("lr");
  });

  it("turns a wide fan-out sideways instead of shrinking it", () => {
    const leaves = ["Почта", "Чат", "Звонок", "Форма", "Бот", "Офис"];
    const layout = layoutFlow({
      kind: "flow",
      direction: "td",
      nodes: [{ id: "root", label: "Клиент" }, ...leaves.map((l) => ({ id: l, label: l }))],
      edges: leaves.map((l) => ({ from: "root", to: l })),
    });
    expect(layout.direction).toBe("lr");
    expect(layout.width).toBeLessThanOrEqual(FLOW_MAX_WIDTH);
    assertNoOverlaps(layout.nodes);
  });

  it("keeps every edge path finite and inside the drawing", () => {
    const layout = layoutFlow({
      kind: "flow",
      nodes: ["a", "b", "c", "d"].map((id) => ({ id, label: id })),
      edges: [
        { from: "a", to: "b", label: "да" },
        { from: "b", to: "c" },
        { from: "c", to: "a", label: "повтор" },
        { from: "d", to: "d" },
        { from: "a", to: "d" },
      ],
    });
    for (const e of layout.edges) {
      const nums = e.path.match(/-?\d+(?:\.\d+)?/g)!.map(Number);
      expect(nums.every(Number.isFinite)).toBe(true);
      expect(Math.min(...nums)).toBeGreaterThanOrEqual(0);
      expect(Math.max(...nums.filter((_, i) => i % 2 === 0))).toBeLessThanOrEqual(layout.width);
    }
  });
});
