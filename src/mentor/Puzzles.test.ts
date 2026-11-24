import { describe, expect, it } from "vitest";
import { makeSudoku, SUDOKU_MODES } from "./Puzzles";

function validSolution(g: number[], n: number, box: [number, number]): boolean {
  const ok = (cells: number[]) => new Set(cells).size === n && cells.every((v) => v >= 1 && v <= n);
  for (let r = 0; r < n; r++) if (!ok(g.slice(r * n, r * n + n))) return false;
  for (let c = 0; c < n; c++) if (!ok(Array.from({ length: n }, (_, r) => g[r * n + c]!))) return false;
  for (let br = 0; br < n; br += box[0])
    for (let bc = 0; bc < n; bc += box[1]) {
      const cells: number[] = [];
      for (let y = br; y < br + box[0]; y++) for (let x = bc; x < bc + box[1]; x++) cells.push(g[y * n + x]!);
      if (!ok(cells)) return false;
    }
  return true;
}

describe("sudoku", () => {
  for (const mode of SUDOKU_MODES) {
    it(`${mode.label}: a valid grid, the clues agree with it`, () => {
      const { puzzle, solution } = makeSudoku(mode);
      expect(validSolution(solution, mode.n, mode.box)).toBe(true);
      const clues = puzzle.filter(Boolean).length;
      expect(clues).toBeGreaterThanOrEqual(mode.clues);
      expect(clues).toBeLessThan(mode.n * mode.n);
      puzzle.forEach((v, i) => v && expect(v).toBe(solution[i]));
    });
  }
});
