// Classic Russian Battleship (Морской бой 10×10) engine + Fair Probability-Density AI
// + spoken Russian/Latin coordinate parser.
// Columns: А Б В Г Д Е Ж З И К (0..9), Rows: 1..10 (0..9).
// Fleet: 1×4 (Линкор), 2×3 (Крейсер), 3×2 (Эсминец), 4×1 (Катер) = 10 ships, 20 decks.
// IMPORTANT: JARVIS's AI NEVER inspects `playerShips` — it sees only its own `jarvisShots`
// grid and the sizes of remaining unsunk ships, playing 100% fairly!

import type { Difficulty } from "./types";

export const BS_COLS = ["А", "Б", "В", "Г", "Д", "Е", "Ж", "З", "И", "К"] as const;
export const FLEET_SPEC = [4, 3, 3, 2, 2, 2, 1, 1, 1, 1] as const;

export type ShotMark = "unknown" | "miss" | "halo" | "hit" | "sunk";

export interface Ship {
  id: number;
  size: number;
  row: number;
  col: number;
  horiz: boolean;
  cells: number[];
  hits: number;
  sunk: boolean;
}

export interface ShotOutcome {
  cell: number;
  coord: string;
  result: "miss" | "hit" | "sunk" | "repeat";
  shipSize?: number;
  shipName?: string;
}

export interface BattleshipState {
  phase: "setup" | "playing" | "finished" | "resigned";
  turn: "player" | "jarvis";
  playerShips: Ship[];
  jarvisShips: Ship[];
  /** Player's shots onto Jarvis's ocean (length 100). */
  playerShots: ShotMark[];
  /** Jarvis's shots onto Player's ocean (length 100). */
  jarvisShots: ShotMark[];
  history: Array<{ by: "player" | "jarvis"; coord: string; result: "miss" | "hit" | "sunk"; shipName?: string }>;
  winner: "player" | "jarvis" | null;
}

export function shipClassRu(size: number): string {
  switch (size) {
    case 4: return "четырёхпалубный линкор";
    case 3: return "трёхпалубный крейсер";
    case 2: return "двухпалубный эсминец";
    default: return "торпедный катер";
  }
}

export function bsCellToCoord(cell: number): string {
  const r = Math.floor(cell / 10);
  const c = cell % 10;
  return `${BS_COLS[c] ?? "А"}${r + 1}`;
}

export function shipCells(row: number, col: number, size: number, horiz: boolean): number[] | null {
  const out: number[] = [];
  for (let k = 0; k < size; k++) {
    const r = horiz ? row : row + k;
    const c = horiz ? col + k : col;
    if (r < 0 || r >= 10 || c < 0 || c >= 10) return null;
    out.push(r * 10 + c);
  }
  return out;
}

export function neighborsWithSelf(cells: number[]): number[] {
  const set = new Set<number>();
  for (const cell of cells) {
    const r = Math.floor(cell / 10);
    const c = cell % 10;
    for (let dr = -1; dr <= 1; dr++) {
      for (let dc = -1; dc <= 1; dc++) {
        const nr = r + dr;
        const nc = c + dc;
        if (nr >= 0 && nr < 10 && nc >= 0 && nc < 10) {
          set.add(nr * 10 + nc);
        }
      }
    }
  }
  return [...set];
}

export function canPlaceShip(
  existing: Ship[],
  row: number,
  col: number,
  size: number,
  horiz: boolean,
  ignoreId?: number,
): boolean {
  const cells = shipCells(row, col, size, horiz);
  if (!cells) return false;
  const forbidden = new Set<number>();
  for (const s of existing) {
    if (s.id === ignoreId) continue;
    for (const n of neighborsWithSelf(s.cells)) forbidden.add(n);
  }
  return cells.every((c) => !forbidden.has(c));
}

export function randomFleet(): Ship[] {
  for (let attempt = 0; attempt < 120; attempt++) {
    const placed: Ship[] = [];
    let ok = true;
    for (let i = 0; i < FLEET_SPEC.length; i++) {
      const size = FLEET_SPEC[i]!;
      const candidates: Array<{ r: number; c: number; h: boolean }> = [];
      for (let r = 0; r < 10; r++) {
        for (let c = 0; c < 10; c++) {
          for (const h of [true, false]) {
            if (size === 1 && !h) continue;
            if (canPlaceShip(placed, r, c, size, h)) {
              candidates.push({ r, c, h });
            }
          }
        }
      }
      if (candidates.length === 0) {
        ok = false;
        break;
      }
      const pick = candidates[Math.floor(Math.random() * candidates.length)]!;
      placed.push({
        id: i + 1,
        size,
        row: pick.r,
        col: pick.c,
        horiz: pick.h,
        cells: shipCells(pick.r, pick.c, size, pick.h)!,
        hits: 0,
        sunk: false,
      });
    }
    if (ok && placed.length === FLEET_SPEC.length) return placed;
  }
  return [];
}

export function createBattleshipState(): BattleshipState {
  return {
    phase: "setup",
    turn: "player",
    playerShips: randomFleet(),
    jarvisShips: randomFleet(),
    playerShots: Array<ShotMark>(100).fill("unknown"),
    jarvisShots: Array<ShotMark>(100).fill("unknown"),
    history: [],
    winner: null,
  };
}

export function startBattleshipMatch(st: BattleshipState): BattleshipState {
  if (st.playerShips.length !== FLEET_SPEC.length) return st;
  return {
    ...st,
    phase: "playing",
    turn: "player",
    jarvisShips: st.jarvisShips.length === FLEET_SPEC.length ? st.jarvisShips : randomFleet(),
    playerShots: Array<ShotMark>(100).fill("unknown"),
    jarvisShots: Array<ShotMark>(100).fill("unknown"),
    history: [],
    winner: null,
  };
}

function resolveShotOnFleet(
  ships: Ship[],
  shots: ShotMark[],
  cell: number,
): { ships: Ship[]; shots: ShotMark[]; outcome: ShotOutcome } {
  const coord = bsCellToCoord(cell);
  if (cell < 0 || cell >= 100 || shots[cell] !== "unknown") {
    return { ships, shots, outcome: { cell, coord, result: "repeat" } };
  }
  const nextShots = shots.slice();
  const nextShips = ships.map((s) => ({ ...s }));
  const hitShip = nextShips.find((s) => s.cells.includes(cell));

  if (!hitShip) {
    nextShots[cell] = "miss";
    return { ships: nextShips, shots: nextShots, outcome: { cell, coord, result: "miss" } };
  }

  hitShip.hits += 1;
  if (hitShip.hits >= hitShip.size) {
    hitShip.sunk = true;
    for (const sc of hitShip.cells) nextShots[sc] = "sunk";
    for (const haloCell of neighborsWithSelf(hitShip.cells)) {
      if (nextShots[haloCell] === "unknown") nextShots[haloCell] = "halo";
    }
    return {
      ships: nextShips,
      shots: nextShots,
      outcome: {
        cell,
        coord,
        result: "sunk",
        shipSize: hitShip.size,
        shipName: shipClassRu(hitShip.size),
      },
    };
  }

  nextShots[cell] = "hit";
  return {
    ships: nextShips,
    shots: nextShots,
    outcome: {
      cell,
      coord,
      result: "hit",
      shipSize: hitShip.size,
      shipName: shipClassRu(hitShip.size),
    },
  };
}

export function firePlayerShot(
  st: BattleshipState,
  cell: number,
): { state: BattleshipState; outcome: ShotOutcome } {
  if (st.phase !== "playing" || st.turn !== "player") {
    return { state: st, outcome: { cell, coord: bsCellToCoord(cell), result: "repeat" } };
  }
  const { ships, shots, outcome } = resolveShotOnFleet(st.jarvisShips, st.playerShots, cell);
  if (outcome.result === "repeat") return { state: st, outcome };

  const allSunk = ships.every((s) => s.sunk);
  const nextTurn = outcome.result === "miss" ? "jarvis" : "player";
  return {
    outcome,
    state: {
      ...st,
      jarvisShips: ships,
      playerShots: shots,
      turn: allSunk ? "player" : nextTurn,
      phase: allSunk ? "finished" : "playing",
      winner: allSunk ? "player" : null,
      history: [
        ...st.history,
        { by: "player", coord: outcome.coord, result: outcome.result, shipName: outcome.shipName },
      ],
    },
  };
}

export function fireJarvisShot(
  st: BattleshipState,
  cell: number,
): { state: BattleshipState; outcome: ShotOutcome } {
  if (st.phase !== "playing" || st.turn !== "jarvis") {
    return { state: st, outcome: { cell, coord: bsCellToCoord(cell), result: "repeat" } };
  }
  const { ships, shots, outcome } = resolveShotOnFleet(st.playerShips, st.jarvisShots, cell);
  if (outcome.result === "repeat") return { state: st, outcome };

  const allSunk = ships.every((s) => s.sunk);
  const nextTurn = outcome.result === "miss" ? "player" : "jarvis";
  return {
    outcome,
    state: {
      ...st,
      playerShips: ships,
      jarvisShots: shots,
      turn: allSunk ? "jarvis" : nextTurn,
      phase: allSunk ? "finished" : "playing",
      winner: allSunk ? "jarvis" : null,
      history: [
        ...st.history,
        { by: "jarvis", coord: outcome.coord, result: outcome.result, shipName: outcome.shipName },
      ],
    },
  };
}

/**
 * 100% Fair Probability-Density AI shooter.
 * Takes ONLY `shots` (what Jarvis has fired so far) and `remainingSizes`
 * (sizes of player ships not yet sunk — public knowledge in Battleship).
 */
export function chooseBattleshipShot(
  shots: ShotMark[],
  remainingSizes: number[],
  difficulty: Difficulty,
): number {
  const unknowns: number[] = [];
  const activeHits: number[] = [];
  for (let i = 0; i < 100; i++) {
    if (shots[i] === "unknown") unknowns.push(i);
    else if (shots[i] === "hit") activeHits.push(i);
  }
  if (unknowns.length === 0) return 0;

  // TARGET MODE: finish damaged ship!
  if (activeHits.length > 0) {
    if (activeHits.length >= 2) {
      const sameRow = activeHits.every((c) => Math.floor(c / 10) === Math.floor(activeHits[0]! / 10));
      const sameCol = activeHits.every((c) => c % 10 === activeHits[0]! % 10);
      if (sameRow) {
        const r = Math.floor(activeHits[0]! / 10);
        const cols = activeHits.map((c) => c % 10).sort((a, b) => a - b);
        const ends = [cols[0]! - 1, cols[cols.length - 1]! + 1]
          .filter((c) => c >= 0 && c < 10 && shots[r * 10 + c] === "unknown")
          .map((c) => r * 10 + c);
        if (ends.length > 0) return ends[Math.floor(Math.random() * ends.length)]!;
      } else if (sameCol) {
        const col = activeHits[0]! % 10;
        const rows = activeHits.map((c) => Math.floor(c / 10)).sort((a, b) => a - b);
        const ends = [rows[0]! - 1, rows[rows.length - 1]! + 1]
          .filter((r) => r >= 0 && r < 10 && shots[r * 10 + col] === "unknown")
          .map((r) => r * 10 + col);
        if (ends.length > 0) return ends[Math.floor(Math.random() * ends.length)]!;
      }
    }

    // Score orthogonal neighbors around active hits
    const orthoCandidates = new Map<number, number>();
    for (const h of activeHits) {
      const r = Math.floor(h / 10);
      const c = h % 10;
      for (const [dr, dc] of [[-1, 0], [1, 0], [0, -1], [0, 1]] as const) {
        const nr = r + dr;
        const nc = c + dc;
        if (nr >= 0 && nr < 10 && nc >= 0 && nc < 10) {
          const idx = nr * 10 + nc;
          if (shots[idx] === "unknown") {
            orthoCandidates.set(idx, (orthoCandidates.get(idx) ?? 0) + 1);
          }
        }
      }
    }

    if (orthoCandidates.size > 0) {
      if (difficulty === "easy") {
        const keys = [...orthoCandidates.keys()];
        return keys[Math.floor(Math.random() * keys.length)]!;
      }
      // Weight by how many remaining ships can fit across the hit + neighbor
      let best = -1;
      let bestWeight = -1;
      for (const cand of orthoCandidates.keys()) {
        let w = 1;
        for (const sz of remainingSizes.filter((s) => s >= 2)) {
          for (let r = 0; r < 10; r++) {
            for (let c = 0; c < 10; c++) {
              for (const horiz of [true, false]) {
                const cells = shipCells(r, c, sz, horiz);
                if (!cells || !cells.includes(cand)) continue;
                if (!activeHits.some((ah) => cells.includes(ah))) continue;
                if (cells.every((x) => shots[x] === "unknown" || shots[x] === "hit")) {
                  w += 4;
                }
              }
            }
          }
        }
        w += Math.random() * 0.5;
        if (w > bestWeight) {
          bestWeight = w;
          best = cand;
        }
      }
      if (best >= 0) return best;
    }
  }

  // HUNT MODE
  if (difficulty === "easy") {
    return unknowns[Math.floor(Math.random() * unknowns.length)]!;
  }

  // Probability density heatmap over all remaining unsunk ship sizes
  const heat = new Float64Array(100);
  const sizes = remainingSizes.length > 0 ? remainingSizes : [1];
  const maxRem = Math.max(...sizes);

  for (const sz of sizes) {
    for (let r = 0; r < 10; r++) {
      for (let c = 0; c < 10; c++) {
        for (const horiz of [true, false]) {
          if (sz === 1 && !horiz) continue;
          const cells = shipCells(r, c, sz, horiz);
          if (!cells) continue;
          if (cells.every((x) => shots[x] === "unknown")) {
            for (const x of cells) heat[x] = (heat[x] ?? 0) + sz;
          }
        }
      }
    }
  }

  if (difficulty === "medium") {
    // Parity filter + slight noise for natural play
    const parity = maxRem >= 2 ? unknowns.filter((i) => (Math.floor(i / 10) + (i % 10)) % 2 === 0) : unknowns;
    const pool = parity.length > 0 ? parity : unknowns;
    pool.sort((a, b) => heat[b]! - heat[a]!);
    const topK = pool.slice(0, Math.min(5, pool.length));
    return topK[Math.floor(Math.random() * topK.length)]!;
  }

  // Hard: pick from peak probability cells
  let maxH = -1;
  for (const u of unknowns) {
    if (heat[u]! > maxH) maxH = heat[u]!;
  }
  const bestCells = unknowns.filter((u) => heat[u]! >= maxH * 0.96);
  return bestCells[Math.floor(Math.random() * bestCells.length)]!;
}

export function battleshipFleetSummary(st: BattleshipState): {
  playerAlive: number;
  jarvisAlive: number;
  text: string;
  safeBoardReport: string;
} {
  const playerAlive = st.playerShips.filter((s) => !s.sunk).length;
  const jarvisAlive = st.jarvisShips.filter((s) => !s.sunk).length;
  const pDamaged = st.playerShips.filter((s) => !s.sunk && s.hits > 0).length;
  const jDamaged = st.jarvisShips.filter((s) => !s.sunk && s.hits > 0).length;
  const text = `Флот игрока: ${playerAlive}/10 · Флот Джарвиса: ${jarvisAlive}/10`;
  const safeBoardReport =
    `Кораблей на плаву — у игрока: ${playerAlive} из 10${pDamaged ? ` (ранено: ${pDamaged})` : ""}, ` +
    `у Джарвиса: ${jarvisAlive} из 10${jDamaged ? ` (ранено: ${jDamaged})` : ""}. ` +
    `Выстрелов сделано: ${st.history.length}.`;
  return { playerAlive, jarvisAlive, text, safeBoardReport };
}

export function hintBattleshipShot(st: BattleshipState): { cell: number; text: string } | null {
  if (st.phase !== "playing") return null;
  const rem = st.jarvisShips.filter((s) => !s.sunk).map((s) => s.size);
  const cell = chooseBattleshipShot(st.playerShots, rem, "hard");
  const coord = bsCellToCoord(cell);
  const hasHit = st.playerShots.includes("hit");
  const why = hasHit
    ? "добивает уже подбитый корабль по линии вероятного корпуса"
    : "накрывает сектор с наибольшей плотностью оставшихся кораблей";
  return { cell, text: `Советую залп в квадрат ${coord} — он ${why}.` };
}

const COL_SYNONYMS: Array<[RegExp, number]> = [
  [/^(а|a)/i, 0],
  [/^(бэ|бе|б|b)/i, 1],
  [/^(вэ|ве|в|c)/i, 2],
  [/^(гэ|ге|г|d)/i, 3],
  [/^(дэ|де|д|e)/i, 4],
  [/^(е|f)/i, 5],
  [/^(жэ|же|ж|g)/i, 6],
  [/^(зэ|зе|з|h)/i, 7],
  [/^(и|i)/i, 8],
  [/^(ка|к|j|k)/i, 9],
];

const NUM_WORDS: Array<[string, number]> = [
  ["десять", 10],
  ["девять", 9],
  ["восемь", 8],
  ["семь", 7],
  ["шесть", 6],
  ["пять", 5],
  ["четыре", 4],
  ["три", 3],
  ["два", 2],
  ["один", 1],
  ["раз", 1],
];

/** Parse spoken/typed Russian or Latin coordinates like "Б5", "к-10", "стреляй в г десять", "ж 7". */
export function parseSpokenBattleshipShot(raw: string): number | null {
  let s = raw.trim().toLowerCase().replace(/ё/g, "е");
  for (const [w, n] of NUM_WORDS) {
    s = s.replace(new RegExp(`(^|[^a-zа-я0-9])(${w})(?=$|[^a-zа-я0-9])`, "gi"), `$1${n}`);
  }
  // Remove leading command words
  s = s.replace(/^(стреляй|пли|огонь|бей|залп|ход|ходи|квадрат|клетка|поле|в|на|по)\s+/g, "").trim();
  s = s.replace(/^(в|на|по)\s+/g, "").trim();

  // Direct regex: letter/syllable + optional separator + 1..10
  const m = s.match(/(бэ|бе|вэ|ве|гэ|ге|дэ|де|жэ|же|зэ|зе|ка|[абвгдежзикabcdefghijk])\s*[-–—:\s]?\s*(10|[1-9])\b/i);
  if (!m) return null;
  const colRaw = m[1]!;
  const rowNum = Number(m[2]!);
  if (rowNum < 1 || rowNum > 10) return null;

  let col = -1;
  const exactIdx = BS_COLS.findIndex((ch) => ch.toLowerCase() === colRaw.toLowerCase());
  if (exactIdx >= 0) {
    col = exactIdx;
  } else {
    for (const [re, idx] of COL_SYNONYMS) {
      if (re.test(colRaw)) {
        col = idx;
        break;
      }
    }
  }
  if (col < 0 || col >= 10) return null;
  return (rowNum - 1) * 10 + col;
}
