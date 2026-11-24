import { describe, expect, it } from "vitest";
import {
  bsCellToCoord,
  chooseBattleshipShot,
  createBattleshipState,
  firePlayerShot,
  parseSpokenBattleshipShot,
  randomFleet,
  startBattleshipMatch,
  type ShotMark,
} from "./battleship";
import {
  applyCheckersMove,
  ckCoordToSq,
  createCheckersState,
  legalCheckersMoves,
  parseSpokenCheckersMove,
  type CheckersState,
} from "./checkers";
import {
  applyChessMove,
  chooseChessMove,
  coordToSq,
  createChessState,
  legalMoves,
  parseSpokenChessMove,
  sqToCoord,
  toFen,
} from "./chess";

describe("Chess engine & spoken move parser", () => {
  it("starts with 20 legal moves and canonical starting FEN", () => {
    const st = createChessState();
    expect(legalMoves(st)).toHaveLength(20);
    expect(toFen(st)).toBe("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1");
  });

  it("recognises openings, en-passant, castling, and Scholar's mate", () => {
    let st = createChessState();
    for (const raw of ["e2e4", "e7e5", "Сc4", "Кc6", "Фh5", "Кf6", "Фf7"]) {
      const m = parseSpokenChessMove(st, raw);
      expect(m, `move ${raw} should be legal`).not.toBeNull();
      st = applyChessMove(st, m!);
    }
    expect(st.status).toBe("checkmate");
    expect(st.winner).toBe("w");
    expect(st.history.at(-1)?.ru).toContain("#");
  });

  it("handles en passant capture accurately", () => {
    let st = createChessState();
    for (const raw of ["e2e4", "a7a6", "e4e5", "d7d5"]) {
      st = applyChessMove(st, parseSpokenChessMove(st, raw)!);
    }
    expect(st.ep).toBe(coordToSq("d6"));
    const epMove = parseSpokenChessMove(st, "e5d6");
    expect(epMove?.enPassant).toBe(true);
    st = applyChessMove(st, epMove!);
    expect(st.board[coordToSq("d5")!]).toBeNull();
    expect(st.board[coordToSq("d6")!]).toEqual({ color: "w", kind: "p" });
  });

  it("parses natural Russian spoken moves and castling", () => {
    let st = createChessState();
    const m1 = parseSpokenChessMove(st, "пешка е два е четыре");
    expect(m1 && `${sqToCoord(m1.from)}${sqToCoord(m1.to)}`).toBe("e2e4");
    st = applyChessMove(st, m1!);
    st = applyChessMove(st, parseSpokenChessMove(st, "c7c5")!);
    expect(st.openingName).toContain("Сицилианская");

    const m2 = parseSpokenChessMove(st, "конём на эф три");
    expect(m2 && `${sqToCoord(m2.from)}${sqToCoord(m2.to)}`).toBe("g1f3");
    st = applyChessMove(st, m2!);
    st = applyChessMove(st, parseSpokenChessMove(st, "d7d6")!);
    st = applyChessMove(st, parseSpokenChessMove(st, "слон с4")!);
    st = applyChessMove(st, parseSpokenChessMove(st, "g8f6")!);

    const castle = parseSpokenChessMove(st, "короткая рокировка");
    expect(castle?.castling).toBe("k");
  });

  it("finds mate-in-1 immediately on hard", () => {
    let st = createChessState();
    for (const raw of ["e2e4", "e7e5", "Сc4", "Кc6", "Фh5", "a7a6"]) {
      st = applyChessMove(st, parseSpokenChessMove(st, raw)!);
    }
    const best = chooseChessMove(st, "hard");
    expect(best).not.toBeNull();
    const after = applyChessMove(st, best!);
    expect(after.status).toBe("checkmate");
  });
});

describe("Russian Checkers (8×8) engine", () => {
  it("starts with 7 white moves and 12 pieces per side", () => {
    const st = createCheckersState();
    expect(legalCheckersMoves(st)).toHaveLength(7);
  });

  it("enforces mandatory backward capture and mid-chain flying king promotion", () => {
    // Place a white man on c5 (row 3, col 2) that must capture backward over d4 -> e3,
    // plus test mid-jump promotion on b6 -> d8(king!) -> g5!
    const board: CheckersState["board"] = Array(64).fill(null);
    board[ckCoordToSq("b6")!] = { color: "w", king: false };
    board[ckCoordToSq("c7")!] = { color: "b", king: false };
    board[ckCoordToSq("f6")!] = { color: "b", king: false };

    const st: CheckersState = {
      board,
      turn: "w",
      history: [],
      quietMoves: 0,
      status: "playing",
      winner: null,
    };

    const moves = legalCheckersMoves(st);
    expect(moves.length).toBeGreaterThan(0);
    // Must promote at d8 and continue capturing f6 as a flying king in the same turn!
    expect(moves.some((m) => m.promoted && m.captures.length === 2)).toBe(true);
  });

  it("parses spoken Russian checkers moves", () => {
    const st = createCheckersState();
    const mv = parseSpokenCheckersMove(st, "цэ три на дэ четыре");
    expect(mv).not.toBeNull();
    const next = applyCheckersMove(st, mv!);
    expect(next.turn).toBe("b");
  });
});

describe("Battleship (10×10) engine & fair AI", () => {
  it("generates a 10-ship fleet (20 cells) with no touching ships", () => {
    const fleet = randomFleet();
    expect(fleet).toHaveLength(10);
    const occupied = new Set<number>();
    for (const s of fleet) {
      for (const c of s.cells) {
        expect(occupied.has(c)).toBe(false);
        occupied.add(c);
      }
    }
    expect(occupied.size).toBe(20);

    // Verify no two distinct ships touch even diagonally
    for (const s1 of fleet) {
      for (const s2 of fleet) {
        if (s1.id === s2.id) continue;
        for (const c1 of s1.cells) {
          const r1 = Math.floor(c1 / 10);
          const col1 = c1 % 10;
          for (const c2 of s2.cells) {
            const r2 = Math.floor(c2 / 10);
            const col2 = c2 % 10;
            expect(Math.max(Math.abs(r1 - r2), Math.abs(col1 - col2))).toBeGreaterThan(1);
          }
        }
      }
    }
  });

  it("marks halo around sunk ship and keeps shooter's turn on hit", () => {
    const st = startBattleshipMatch(createBattleshipState());
    const oneDeck = st.jarvisShips.find((s) => s.size === 1)!;
    const cell = oneDeck.cells[0]!;
    const { state: next, outcome } = firePlayerShot(st, cell);
    expect(outcome.result).toBe("sunk");
    expect(next.turn).toBe("player");
    expect(next.playerShots[cell]).toBe("sunk");

    const r = Math.floor(cell / 10);
    const c = cell % 10;
    for (let dr = -1; dr <= 1; dr++) {
      for (let dc = -1; dc <= 1; dc++) {
        if (dr === 0 && dc === 0) continue;
        const nr = r + dr;
        const nc = c + dc;
        if (nr >= 0 && nr < 10 && nc >= 0 && nc < 10) {
          expect(next.playerShots[nr * 10 + nc]).toBe("halo");
        }
      }
    }
  });

  it("targets collinear neighbors when finishing a multi-deck ship", () => {
    const shots: ShotMark[] = Array(100).fill("unknown");
    shots[44] = "hit"; // Д5
    shots[45] = "hit"; // Е5
    const pick = chooseBattleshipShot(shots, [4, 3, 2, 1], "hard");
    expect([43, 46]).toContain(pick);
  });

  it("parses Russian spoken coordinates including К10 and word numbers", () => {
    expect(bsCellToCoord(parseSpokenBattleshipShot("Б5")!)).toBe("Б5");
    expect(bsCellToCoord(parseSpokenBattleshipShot("стреляй в г десять")!)).toBe("Г10");
    expect(bsCellToCoord(parseSpokenBattleshipShot("пли ка 10")!)).toBe("К10");
    expect(bsCellToCoord(parseSpokenBattleshipShot("жэ семь")!)).toBe("Ж7");
  });
});
