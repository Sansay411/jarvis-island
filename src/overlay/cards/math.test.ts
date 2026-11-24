import { describe, expect, it } from "vitest";
import { prettyMath } from "./math";

describe("prettyMath", () => {
  it("raises powers and lowers indices", () => {
    expect(prettyMath("x^2 + y^{10}")).toBe("x² + y¹⁰");
    expect(prettyMath("a_1 + a_n")).toBe("a₁ + aₙ");
    expect(prettyMath(String.raw`e^{i\pi}`)).toBe("e^(iπ)");
  });

  it("turns common LaTeX into plain symbols", () => {
    expect(prettyMath(String.raw`$\frac{a+b}{2} \cdot \sqrt{16} \le 10$`)).toBe("(a+b)/2 · √16 ≤ 10");
    expect(prettyMath(String.raw`\Delta = b^2 - 4ac`)).toBe("Δ = b² − 4ac");
    expect(prettyMath(String.raw`x = \frac{-b \pm \sqrt{D}}{2a}`)).toBe("x = (−b ± √D)/(2a)");
  });

  it("prettifies ASCII operators", () => {
    expect(prettyMath("2*x >= 8 -> x >= 4")).toBe("2 · x ≥ 8 → x ≥ 4");
    expect(prettyMath("a != b, x=-3")).toBe("a ≠ b, x=−3");
    expect(prettyMath("sqrt(25) = 5")).toBe("√25 = 5");
  });

  it("leaves plain text alone", () => {
    expect(prettyMath("Ответ: 42")).toBe("Ответ: 42");
  });
});
