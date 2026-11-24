// Russian Checkers (Русские шашки 8×8) engine + Alpha-Beta AI + spoken move parser.
// Rules:
// - 12 white vs 12 black pieces on dark squares ((r + c) % 2 === 1).
// - Men move diagonally forward 1 square, capture diagonally BOTH forward & backward.
// - Mandatory capture (бить обязательно).
// - Mid-jump promotion: reaching the back rank during a capture immediately crowns
//   a King (Дамка) and continues jumping in the same turn if King captures exist.
// - Flying Kings move and capture across any number of empty diagonal squares.
//   Captured pieces remain on the board as obstacles until the full jump sequence ends
//   (cannot jump the same piece twice in one turn).

import type { Difficulty } from "./types";

export type CheckersColor = "w" | "b";

export interface CheckerPiece {
  color: CheckersColor;
  king: boolean;
}

export interface CheckersMove {
  from: number;
  to: number;
  /** Full path of squares visited after `from` (length > 1 for multi-jumps). */
  path: number[];
  /** Squares of enemy pieces captured along the sequence. */
  captures: number[];
  promoted: boolean;
}

export interface CheckersHistoryItem {
  move: CheckersMove;
  notation: string;
  color: CheckersColor;
}

export interface CheckersState {
  board: Array<CheckerPiece | null>;
  turn: CheckersColor;
  history: CheckersHistoryItem[];
  quietMoves: number;
  status: "playing" | "finished" | "draw" | "resigned";
  winner: CheckersColor | "draw" | null;
}

const FILES = "abcdefgh";
const DIAGS: Array<[number, number]> = [
  [-1, -1],
  [-1, 1],
  [1, -1],
  [1, 1],
];

export function ckSqToCoord(sq: number): string {
  const r = Math.floor(sq / 8);
  const c = sq % 8;
  return `${FILES[c]}${8 - r}`;
}

export function ckCoordToSq(coord: string): number | null {
  const s = coord.trim().toLowerCase();
  if (s.length !== 2) return null;
  const c = FILES.indexOf(s[0]!);
  const rank = Number(s[1]);
  if (c < 0 || rank < 1 || rank > 8) return null;
  return (8 - rank) * 8 + c;
}

export function formatCheckersMove(m: CheckersMove): string {
  const sep = m.captures.length > 0 ? ":" : "-";
  return [ckSqToCoord(m.from), ...m.path.map(ckSqToCoord)].join(sep);
}

export function createCheckersState(): CheckersState {
  const board: Array<CheckerPiece | null> = Array(64).fill(null);
  for (let r = 0; r < 8; r++) {
    for (let c = 0; c < 8; c++) {
      if ((r + c) % 2 !== 1) continue;
      if (r < 3) board[r * 8 + c] = { color: "b", king: false };
      else if (r > 4) board[r * 8 + c] = { color: "w", king: false };
    }
  }
  return {
    board,
    turn: "w",
    history: [],
    quietMoves: 0,
    status: "playing",
    winner: null,
  };
}

function opp(c: CheckersColor): CheckersColor {
  return c === "w" ? "b" : "w";
}

function isPromoRow(color: CheckersColor, row: number): boolean {
  return color === "w" ? row === 0 : row === 7;
}

/** Generate all capture chains starting at `sq` for piece `(color, king)`. */
function captureChainsFrom(
  board: Array<CheckerPiece | null>,
  sq: number,
  color: CheckersColor,
  king: boolean,
  originSq: number,
  alreadyCaptured: number[],
  pathSoFar: number[],
  promotedSoFar: boolean,
): CheckersMove[] {
  const r = Math.floor(sq / 8);
  const c = sq % 8;
  const enemy = opp(color);
  const continuations: CheckersMove[] = [];

  if (!king) {
    for (const [dr, dc] of DIAGS) {
      const mr = r + dr;
      const mc = c + dc;
      const lr = r + dr * 2;
      const lc = c + dc * 2;
      if (lr < 0 || lr >= 8 || lc < 0 || lc >= 8) continue;
      const midSq = mr * 8 + mc;
      const landSq = lr * 8 + lc;
      if (alreadyCaptured.includes(midSq)) continue;
      const midP = board[midSq];
      if (!midP || midP.color !== enemy) continue;
      if (board[landSq] !== null && landSq !== originSq) continue;

      const nowKing = isPromoRow(color, lr);
      const nextCaps = [...alreadyCaptured, midSq];
      const nextPath = [...pathSoFar, landSq];
      const sub = captureChainsFrom(
        board,
        landSq,
        color,
        nowKing,
        originSq,
        nextCaps,
        nextPath,
        promotedSoFar || nowKing,
      );
      if (sub.length > 0) continuations.push(...sub);
      else {
        continuations.push({
          from: originSq,
          to: landSq,
          path: nextPath,
          captures: nextCaps,
          promoted: promotedSoFar || nowKing,
        });
      }
    }
  } else {
    // Flying King capture
    for (const [dr, dc] of DIAGS) {
      let nr = r + dr;
      let nc = c + dc;
      let hitSq = -1;
      while (nr >= 0 && nr < 8 && nc >= 0 && nc < 8) {
        const cur = nr * 8 + nc;
        if (alreadyCaptured.includes(cur)) break;
        const p = cur === originSq ? null : board[cur];
        if (p) {
          if (p.color === enemy) hitSq = cur;
          break;
        }
        nr += dr;
        nc += dc;
      }
      if (hitSq < 0) continue;

      // Explore all empty landing squares past hitSq along the same diagonal
      let lr = Math.floor(hitSq / 8) + dr;
      let lc = (hitSq % 8) + dc;
      const landingsWithSub: CheckersMove[] = [];
      const terminalLandings: CheckersMove[] = [];

      while (lr >= 0 && lr < 8 && lc >= 0 && lc < 8) {
        const landSq = lr * 8 + lc;
        if (alreadyCaptured.includes(landSq)) break;
        const occ = landSq === originSq ? null : board[landSq];
        if (occ) break;

        const nextCaps = [...alreadyCaptured, hitSq];
        const nextPath = [...pathSoFar, landSq];
        const sub = captureChainsFrom(board, landSq, color, true, originSq, nextCaps, nextPath, promotedSoFar);
        if (sub.length > 0) {
          landingsWithSub.push(...sub);
        } else {
          terminalLandings.push({
            from: originSq,
            to: landSq,
            path: nextPath,
            captures: nextCaps,
            promoted: promotedSoFar,
          });
        }
        lr += dr;
        lc += dc;
      }
      // Russian checkers rule: if any landing along this diagonal allows continuing the jump, must take one of those
      if (landingsWithSub.length > 0) continuations.push(...landingsWithSub);
      else continuations.push(...terminalLandings);
    }
  }

  return continuations;
}

export function legalCheckersMoves(st: CheckersState): CheckersMove[] {
  if (st.status !== "playing") return [];
  return legalCheckersMovesBoard(st.board, st.turn);
}

function legalCheckersMovesBoard(board: Array<CheckerPiece | null>, turn: CheckersColor): CheckersMove[] {
  const captures: CheckersMove[] = [];
  for (let sq = 0; sq < 64; sq++) {
    const p = board[sq];
    if (!p || p.color !== turn) continue;
    captures.push(...captureChainsFrom(board, sq, turn, p.king, sq, [], [], false));
  }
  if (captures.length > 0) return captures;

  // Quiet moves when no capture exists
  const quiet: CheckersMove[] = [];
  for (let sq = 0; sq < 64; sq++) {
    const p = board[sq];
    if (!p || p.color !== turn) continue;
    const r = Math.floor(sq / 8);
    const c = sq % 8;

    if (!p.king) {
      const dr = turn === "w" ? -1 : 1;
      const nr = r + dr;
      if (nr < 0 || nr >= 8) continue;
      for (const dc of [-1, 1]) {
        const nc = c + dc;
        if (nc < 0 || nc >= 8) continue;
        const to = nr * 8 + nc;
        if (!board[to]) {
          quiet.push({
            from: sq,
            to,
            path: [to],
            captures: [],
            promoted: isPromoRow(turn, nr),
          });
        }
      }
    } else {
      for (const [dr, dc] of DIAGS) {
        let nr = r + dr;
        let nc = c + dc;
        while (nr >= 0 && nr < 8 && nc >= 0 && nc < 8) {
          const to = nr * 8 + nc;
          if (board[to]) break;
          quiet.push({ from: sq, to, path: [to], captures: [], promoted: false });
          nr += dr;
          nc += dc;
        }
      }
    }
  }
  return quiet;
}

function applyCheckersBoard(board: Array<CheckerPiece | null>, m: CheckersMove): Array<CheckerPiece | null> {
  const next = board.slice();
  const piece = next[m.from];
  if (!piece) return next;
  next[m.from] = null;
  for (const cap of m.captures) next[cap] = null;
  next[m.to] = { color: piece.color, king: piece.king || m.promoted };
  return next;
}

export function applyCheckersMove(st: CheckersState, m: CheckersMove): CheckersState {
  const nextBoard = applyCheckersBoard(st.board, m);
  const nextTurn = opp(st.turn);
  const quietMoves = m.captures.length === 0 && !m.promoted ? st.quietMoves + 1 : 0;
  const history: CheckersHistoryItem[] = [
    ...st.history,
    { move: m, notation: formatCheckersMove(m), color: st.turn },
  ];

  const replyMoves = legalCheckersMovesBoard(nextBoard, nextTurn);
  let status: CheckersState["status"] = "playing";
  let winner: CheckersState["winner"] = null;
  if (replyMoves.length === 0) {
    status = "finished";
    winner = st.turn;
  } else if (quietMoves >= 30) {
    status = "draw";
    winner = "draw";
  }

  return {
    board: nextBoard,
    turn: nextTurn,
    history,
    quietMoves,
    status,
    winner,
  };
}

export function checkersScoreSummary(st: CheckersState): { wMen: number; wKings: number; bMen: number; bKings: number; text: string } {
  let wMen = 0, wKings = 0, bMen = 0, bKings = 0;
  for (const p of st.board) {
    if (!p) continue;
    if (p.color === "w") {
      if (p.king) wKings++;
      else wMen++;
    } else {
      if (p.king) bKings++;
      else bMen++;
    }
  }
  const wTotal = wMen + wKings;
  const bTotal = bMen + bKings;
  return {
    wMen,
    wKings,
    bMen,
    bKings,
    text: `Белые: ${wTotal}${wKings ? ` (дамок ${wKings})` : ""} · Чёрные: ${bTotal}${bKings ? ` (дамок ${bKings})` : ""}`,
  };
}

export function checkersBoardText(st: CheckersState): string {
  const w: string[] = [];
  const b: string[] = [];
  for (let sq = 0; sq < 64; sq++) {
    const p = st.board[sq];
    if (!p) continue;
    const tag = `${p.king ? "Д" : ""}${ckSqToCoord(sq)}`;
    if (p.color === "w") w.push(tag);
    else b.push(tag);
  }
  return `Белые [${w.join(",")}] | Чёрные [${b.join(",")}]`;
}

function evalCheckers(board: Array<CheckerPiece | null>): number {
  let score = 0;
  for (let sq = 0; sq < 64; sq++) {
    const p = board[sq];
    if (!p) continue;
    const r = Math.floor(sq / 8);
    const c = sq % 8;
    const centerBonus = c >= 2 && c <= 5 && r >= 2 && r <= 5 ? 12 : c >= 1 && c <= 6 ? 5 : 0;
    let val: number;
    if (p.king) {
      val = 310 + centerBonus * 1.5;
    } else {
      const adv = p.color === "w" ? 7 - r : r;
      const golden = (p.color === "w" && sq === 59) || (p.color === "b" && sq === 4) ? 14 : 0;
      val = 100 + adv * 6 + centerBonus + golden;
    }
    score += p.color === "w" ? val : -val;
  }
  return score;
}

function searchCheckers(
  board: Array<CheckerPiece | null>,
  turn: CheckersColor,
  depth: number,
  alpha: number,
  beta: number,
  deadline: number,
): number {
  const moves = legalCheckersMovesBoard(board, turn);
  if (moves.length === 0) return -10000 - depth * 10;
  if (depth <= 0 || performance.now() > deadline) {
    return evalCheckers(board) * (turn === "w" ? 1 : -1);
  }

  moves.sort((a, b) => b.captures.length - a.captures.length || (b.promoted ? 1 : 0) - (a.promoted ? 1 : 0));

  for (const m of moves) {
    const nb = applyCheckersBoard(board, m);
    const ext = m.captures.length >= 2 && depth === 1 ? 1 : 0;
    const val = -searchCheckers(nb, opp(turn), depth - 1 + ext, -beta, -alpha, deadline);
    if (val >= beta) return beta;
    if (val > alpha) alpha = val;
  }
  return alpha;
}

export function chooseCheckersMove(st: CheckersState, difficulty: Difficulty): CheckersMove | null {
  const moves = legalCheckersMoves(st);
  if (moves.length === 0) return null;
  if (moves.length === 1) return moves[0]!;

  const depth = difficulty === "easy" ? 2 : difficulty === "medium" ? 4 : 6;
  const deadline = performance.now() + (difficulty === "easy" ? 80 : difficulty === "medium" ? 180 : 360);

  const scored: Array<{ m: CheckersMove; s: number }> = [];
  for (const m of moves) {
    const nb = applyCheckersBoard(st.board, m);
    const s = -searchCheckers(nb, opp(st.turn), depth - 1, -20000, 20000, deadline);
    scored.push({ m, s });
  }
  scored.sort((a, b) => b.s - a.s);

  if (difficulty === "easy" && scored.length >= 2 && Math.random() < 0.4) {
    return scored[Math.min(scored.length - 1, Math.floor(Math.random() * 3))]!.m;
  }
  if (difficulty === "medium" && scored.length >= 2) {
    const top = scored[0]!.s;
    const close = scored.filter((x) => top - x.s <= 15);
    return close[Math.floor(Math.random() * close.length)]!.m;
  }
  return scored[0]!.m;
}

export function hintCheckersMove(st: CheckersState): { move: CheckersMove; text: string } | null {
  const m = chooseCheckersMove(st, "hard");
  if (!m) return null;
  const notation = formatCheckersMove(m);
  let why = "держит центр и темп";
  if (m.captures.length >= 2) why = `забирает сразу ${m.captures.length} шашки серией прыжков!`;
  else if (m.captures.length === 1 && m.promoted) why = "бьёт шашку и сразу выводит дамку!";
  else if (m.captures.length === 1) why = "выполняет обязательное взятие в выгодную сторону";
  else if (m.promoted) why = "проводит шашку в дамки!";
  return { move: m, text: `Хороший ход: ${notation} — он ${why}.` };
}

export function describeCheckersExchange(
  afterPlayer: CheckersState,
  afterJarvis: CheckersState | null,
): string[] {
  const notes: string[] = [];
  const pLast = afterPlayer.history.at(-1);
  if (pLast) {
    if (pLast.move.captures.length >= 2) {
      notes.push(`Игрок провёл красивую комбинацию и срубил ${pLast.move.captures.length} шашки за ход!`);
    } else if (pLast.move.captures.length === 1) {
      notes.push("Игрок побил шашку Джарвиса");
    }
    if (pLast.move.promoted) notes.push("Игрок прорвался в дамки!");
    if (afterPlayer.winner === "w") notes.push("Игрок победил в шашки!");
  }
  if (afterJarvis) {
    const jLast = afterJarvis.history.at(-1);
    if (jLast) {
      if (jLast.move.captures.length >= 2) {
        notes.push(`Джарвис устроил каскадный разгром: побил сразу ${jLast.move.captures.length} шашки!`);
      } else if (jLast.move.captures.length === 1) {
        notes.push("Джарвис срубил шашку игрока");
      }
      if (jLast.move.promoted) notes.push("Джарвис провёл дамку!");
    }
    if (afterJarvis.winner === "b") notes.push("Джарвис выиграл партию в шашки!");
    else if (afterJarvis.winner === "draw") notes.push("Партия в шашки завершилась ничьей");
  }
  return notes;
}

const RU_FILE_MAP: Record<string, string> = {
  а: "a", бэ: "b", бе: "b", б: "b", в: "b", цэ: "c", це: "c", с: "c", ц: "c", дэ: "d", де: "d", д: "d", е: "e", эф: "f", ф: "f", жэ: "g", же: "g", ж: "g", г: "g", аш: "h", х: "h",
};
const RU_RANK_WORDS: Record<string, string> = {
  один: "1", раз: "1", два: "2", три: "3", четыре: "4", пять: "5", шесть: "6", семь: "7", восемь: "8",
};

export function parseSpokenCheckersMove(st: CheckersState, raw: string): CheckersMove | null {
  const legal = legalCheckersMoves(st);
  if (legal.length === 0) return null;
  let norm = raw.trim().toLowerCase().replace(/ё/g, "е");
  for (const [w, d] of Object.entries(RU_RANK_WORDS)) {
    norm = norm.replace(new RegExp(`(^|[^a-zа-я0-9])(${w})(?=$|[^a-zа-я0-9])`, "gi"), `$1${d}`);
  }
  norm = norm.replace(
    /(аш|эф|бэ|бе|цэ|це|дэ|де|жэ|же|[абвсцдефжгх])\s*([1-8])/g,
    (_, f: string, d: string) => `${RU_FILE_MAP[f] ?? f}${d}`,
  );

  const sqs = [...norm.matchAll(/([a-h][1-8])/g)].map((m) => ckCoordToSq(m[1]!)).filter((x): x is number => x !== null);
  if (sqs.length >= 2) {
    const from = sqs[0]!;
    const to = sqs[sqs.length - 1]!;
    const exact = legal.find(
      (m) => m.from === from && m.to === to && (sqs.length === 2 || m.path.every((p, idx) => sqs[idx + 1] === p)),
    );
    if (exact) return exact;
    return legal.find((m) => m.from === from && m.to === to) ?? null;
  }
  if (sqs.length === 1) {
    const to = sqs[0]!;
    const hits = legal.filter((m) => m.to === to);
    if (hits.length === 1) return hits[0]!;
  }
  return null;
}
