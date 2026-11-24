// Complete FIDE Chess engine + Alpha-Beta AI + Russian/UCI spoken move parser
// + opening recognizer & tactical event extractor for JARVIS's live commentary.

import type { Difficulty } from "./types";

export type ChessColor = "w" | "b";
export type PieceKind = "p" | "n" | "b" | "r" | "q" | "k";

export interface ChessPiece {
  color: ChessColor;
  kind: PieceKind;
}

export interface CastlingRights {
  wk: boolean;
  wq: boolean;
  bk: boolean;
  bq: boolean;
}

export interface ChessMove {
  from: number;
  to: number;
  promotion?: PieceKind;
  castling?: "k" | "q";
  enPassant?: boolean;
}

export interface ChessHistoryItem {
  move: ChessMove;
  uci: string;
  ru: string;
  color: ChessColor;
  captured?: PieceKind;
  check: boolean;
  checkmate: boolean;
}

export interface ChessState {
  board: Array<ChessPiece | null>;
  turn: ChessColor;
  castling: CastlingRights;
  ep: number | null;
  halfmove: number;
  fullmove: number;
  history: ChessHistoryItem[];
  posCounts: Record<string, number>;
  capturedByWhite: PieceKind[];
  capturedByBlack: PieceKind[];
  openingName: string | null;
  status: "playing" | "checkmate" | "stalemate" | "draw_50" | "draw_rep" | "draw_material" | "resigned";
  winner: ChessColor | "draw" | null;
  inCheck: boolean;
}

const FILES = "abcdefgh";
export const PIECE_VALUE: Record<PieceKind, number> = {
  p: 100,
  n: 320,
  b: 335,
  r: 500,
  q: 900,
  k: 20000,
};

export const PIECE_RU_NAME: Record<PieceKind, string> = {
  p: "пешка",
  n: "конь",
  b: "слон",
  r: "ладья",
  q: "ферзь",
  k: "король",
};

const PIECE_RU_LETTER: Record<PieceKind, string> = {
  p: "",
  n: "К",
  b: "С",
  r: "Л",
  q: "Ф",
  k: "Кр",
};

// Piece-square tables (from White's perspective, index 0 = a8 .. 63 = h1)
const PST_PAWN = [
   0,  0,  0,  0,  0,  0,  0,  0,
  50, 50, 50, 50, 50, 50, 50, 50,
  12, 12, 22, 32, 32, 22, 12, 12,
   6,  6, 12, 27, 27, 12,  6,  6,
   0,  0,  4, 24, 24,  4,  0,  0,
   5, -4, -8,  2,  2, -8, -4,  5,
   5, 10, 10,-20,-20, 10, 10,  5,
   0,  0,  0,  0,  0,  0,  0,  0,
];

const PST_KNIGHT = [
 -50,-40,-30,-30,-30,-30,-40,-50,
 -40,-20,  0,  0,  0,  0,-20,-40,
 -30,  0, 12, 16, 16, 12,  0,-30,
 -30,  6, 16, 22, 22, 16,  6,-30,
 -30,  0, 16, 22, 22, 16,  0,-30,
 -30,  6, 12, 16, 16, 12,  6,-30,
 -40,-20,  0,  5,  5,  0,-20,-40,
 -50,-40,-30,-30,-30,-30,-40,-50,
];

const PST_BISHOP = [
 -20,-10,-10,-10,-10,-10,-10,-20,
 -10,  0,  0,  0,  0,  0,  0,-10,
 -10,  0,  8, 12, 12,  8,  0,-10,
 -10,  6,  6, 12, 12,  6,  6,-10,
 -10,  0, 12, 12, 12, 12,  0,-10,
 -10, 10, 10, 10, 10, 10, 10,-10,
 -10,  6,  0,  0,  0,  0,  6,-10,
 -20,-10,-10,-10,-10,-10,-10,-20,
];

const PST_ROOK = [
   0,  0,  0,  4,  4,  0,  0,  0,
   6, 12, 12, 12, 12, 12, 12,  6,
  -5,  0,  0,  0,  0,  0,  0, -5,
  -5,  0,  0,  0,  0,  0,  0, -5,
  -5,  0,  0,  0,  0,  0,  0, -5,
  -5,  0,  0,  0,  0,  0,  0, -5,
  -5,  0,  0,  0,  0,  0,  0, -5,
   0,  0,  2,  8,  8,  2,  0,  0,
];

const PST_QUEEN = [
 -20,-10,-10, -5, -5,-10,-10,-20,
 -10,  0,  0,  0,  0,  0,  0,-10,
 -10,  0,  5,  5,  5,  5,  0,-10,
  -5,  0,  5,  6,  6,  5,  0, -5,
   0,  0,  5,  6,  6,  5,  0, -5,
 -10,  5,  5,  5,  5,  5,  0,-10,
 -10,  0,  5,  0,  0,  0,  0,-10,
 -20,-10,-10, -5, -5,-10,-10,-20,
];

const PST_KING_MID = [
 -30,-40,-40,-50,-50,-40,-40,-30,
 -30,-40,-40,-50,-50,-40,-40,-30,
 -30,-40,-40,-50,-50,-40,-40,-30,
 -30,-40,-40,-50,-50,-40,-40,-30,
 -20,-30,-30,-40,-40,-30,-30,-20,
 -10,-20,-20,-20,-20,-20,-20,-10,
  20, 20,  0,  0,  0,  0, 20, 20,
  20, 32, 12,  0,  0, 12, 32, 20,
];

const PST_KING_END = [
 -50,-40,-30,-20,-20,-30,-40,-50,
 -30,-20,-10,  0,  0,-10,-20,-30,
 -30,-10, 20, 30, 30, 20,-10,-30,
 -30,-10, 30, 40, 40, 30,-10,-30,
 -30,-10, 30, 40, 40, 30,-10,-30,
 -30,-10, 20, 30, 30, 20,-10,-30,
 -30,-30,  0,  0,  0,  0,-30,-30,
 -50,-30,-30,-30,-30,-30,-30,-50,
];

// Recognised classic openings by UCI move prefix
const OPENINGS: Array<[string, string]> = [
  ["e2e4 c7c5", "Сицилианская защита"],
  ["e2e4 e7e5 g1f3 b8c6 f1b5", "Испанская партия"],
  ["e2e4 e7e5 g1f3 b8c6 f1c4", "Итальянская партия"],
  ["e2e4 e7e5 g1f3 g8f6", "Русская партия"],
  ["e2e4 e7e5 f2f4", "Королевский гамбит"],
  ["e2e4 e7e6", "Французская защита"],
  ["e2e4 c7c6", "Защита Каро-Канн"],
  ["e2e4 d7d6", "Защита Пирца-Уфимцева"],
  ["e2e4 d7d5", "Скандинавская защита"],
  ["d2d4 d7d5 c2c4 e7e6", "Отказанный ферзевый гамбит"],
  ["d2d4 d7d5 c2c4 d5c4", "Принятый ферзевый гамбит"],
  ["d2d4 d7d5 c2c4 c7c6", "Славянская защита"],
  ["d2d4 d7d5 c2c4", "Ферзевый гамбит"],
  ["d2d4 g8f6 c2c4 g7g6", "Староиндийская защита"],
  ["d2d4 g8f6 c2c4 e7e6", "Защита Нимцовича"],
  ["c2c4", "Английское начало"],
  ["g1f3 d7d5", "Дебют Рети"],
];

// Opening book responses for lively, varied play
const BOOK: Record<string, string[]> = {
  "": ["e2e4", "d2d4", "c2c4", "g1f3"],
  "e2e4": ["c7c5", "e7e5", "e7e6", "c7c6"],
  "d2d4": ["d7d5", "g8f6"],
  "c2c4": ["e7e5", "g8f6", "c7c5"],
  "g1f3": ["d7d5", "g8f6", "c7c5"],
  "e2e4 e7e5": ["g1f3", "f1c4"],
  "e2e4 c7c5": ["g1f3", "b1c3"],
  "e2e4 e7e5 g1f3": ["b8c6", "g8f6"],
  "e2e4 c7c5 g1f3": ["d7d6", "b8c6", "e7e6"],
  "e2e4 e7e6": ["d2d4"],
  "e2e4 e7e6 d2d4": ["d7d5"],
  "e2e4 c7c6": ["d2d4"],
  "e2e4 c7c6 d2d4": ["d7d5"],
  "d2d4 d7d5": ["c2c4", "g1f3"],
  "d2d4 d7d5 c2c4": ["e7e6", "c7c6"],
  "d2d4 g8f6": ["c2c4", "g1f3"],
  "d2d4 g8f6 c2c4": ["e7e6", "g7g6"],
  "e2e4 e7e5 g1f3 b8c6 f1c4": ["f8c5", "g8f6"],
  "e2e4 e7e5 g1f3 b8c6 f1b5": ["a7a6", "g8f6"],
};

export function sqToCoord(sq: number): string {
  const r = Math.floor(sq / 8);
  const c = sq % 8;
  return `${FILES[c]}${8 - r}`;
}

export function coordToSq(coord: string): number | null {
  const s = coord.trim().toLowerCase();
  if (s.length !== 2) return null;
  const c = FILES.indexOf(s[0]!);
  const rank = Number(s[1]);
  if (c < 0 || rank < 1 || rank > 8) return null;
  return (8 - rank) * 8 + c;
}

export function moveToUci(m: ChessMove): string {
  return `${sqToCoord(m.from)}${sqToCoord(m.to)}${m.promotion ?? ""}`;
}

function posKey(board: Array<ChessPiece | null>, turn: ChessColor, castling: CastlingRights, ep: number | null): string {
  let s = "";
  for (let i = 0; i < 64; i++) {
    const p = board[i];
    s += p ? (p.color === "w" ? p.kind.toUpperCase() : p.kind) : ".";
  }
  s += `|${turn}|${castling.wk ? "K" : ""}${castling.wq ? "Q" : ""}${castling.bk ? "k" : ""}${castling.bq ? "q" : ""}|${ep ?? "-"}`;
  return s;
}

export function createChessState(): ChessState {
  const back: PieceKind[] = ["r", "n", "b", "q", "k", "b", "n", "r"];
  const board: Array<ChessPiece | null> = Array(64).fill(null);
  for (let c = 0; c < 8; c++) {
    board[c] = { color: "b", kind: back[c]! };
    board[8 + c] = { color: "b", kind: "p" };
    board[48 + c] = { color: "w", kind: "p" };
    board[56 + c] = { color: "w", kind: back[c]! };
  }
  const castling: CastlingRights = { wk: true, wq: true, bk: true, bq: true };
  const key = posKey(board, "w", castling, null);
  return {
    board,
    turn: "w",
    castling,
    ep: null,
    halfmove: 0,
    fullmove: 1,
    history: [],
    posCounts: { [key]: 1 },
    capturedByWhite: [],
    capturedByBlack: [],
    openingName: null,
    status: "playing",
    winner: null,
    inCheck: false,
  };
}

export function toFen(st: ChessState): string {
  const rows: string[] = [];
  for (let r = 0; r < 8; r++) {
    let empty = 0;
    let row = "";
    for (let c = 0; c < 8; c++) {
      const p = st.board[r * 8 + c];
      if (!p) {
        empty++;
      } else {
        if (empty > 0) {
          row += String(empty);
          empty = 0;
        }
        row += p.color === "w" ? p.kind.toUpperCase() : p.kind;
      }
    }
    if (empty > 0) row += String(empty);
    rows.push(row);
  }
  const cr = `${st.castling.wk ? "K" : ""}${st.castling.wq ? "Q" : ""}${st.castling.bk ? "k" : ""}${st.castling.bq ? "q" : ""}` || "-";
  const ep = st.ep !== null ? sqToCoord(st.ep) : "-";
  return `${rows.join("/")} ${st.turn} ${cr} ${ep} ${st.halfmove} ${st.fullmove}`;
}

function enemy(c: ChessColor): ChessColor {
  return c === "w" ? "b" : "w";
}

export function isSquareAttacked(board: Array<ChessPiece | null>, sq: number, by: ChessColor): boolean {
  const r = Math.floor(sq / 8);
  const c = sq % 8;

  // Pawn attacks
  const pr = by === "w" ? r + 1 : r - 1;
  if (pr >= 0 && pr < 8) {
    for (const dc of [-1, 1]) {
      const pc = c + dc;
      if (pc >= 0 && pc < 8) {
        const p = board[pr * 8 + pc];
        if (p && p.color === by && p.kind === "p") return true;
      }
    }
  }

  // Knight attacks
  const kSteps: Array<[number, number]> = [
    [-2, -1], [-2, 1], [-1, -2], [-1, 2],
    [1, -2], [1, 2], [2, -1], [2, 1],
  ];
  for (const [dr, dc] of kSteps) {
    const nr = r + dr;
    const nc = c + dc;
    if (nr >= 0 && nr < 8 && nc >= 0 && nc < 8) {
      const p = board[nr * 8 + nc];
      if (p && p.color === by && p.kind === "n") return true;
    }
  }

  // King attacks
  for (let dr = -1; dr <= 1; dr++) {
    for (let dc = -1; dc <= 1; dc++) {
      if (dr === 0 && dc === 0) continue;
      const nr = r + dr;
      const nc = c + dc;
      if (nr >= 0 && nr < 8 && nc >= 0 && nc < 8) {
        const p = board[nr * 8 + nc];
        if (p && p.color === by && p.kind === "k") return true;
      }
    }
  }

  // Orthogonal rays (Rook / Queen)
  const ortho: Array<[number, number]> = [[-1, 0], [1, 0], [0, -1], [0, 1]];
  for (const [dr, dc] of ortho) {
    let nr = r + dr;
    let nc = c + dc;
    while (nr >= 0 && nr < 8 && nc >= 0 && nc < 8) {
      const p = board[nr * 8 + nc];
      if (p) {
        if (p.color === by && (p.kind === "r" || p.kind === "q")) return true;
        break;
      }
      nr += dr;
      nc += dc;
    }
  }

  // Diagonal rays (Bishop / Queen)
  const diag: Array<[number, number]> = [[-1, -1], [-1, 1], [1, -1], [1, 1]];
  for (const [dr, dc] of diag) {
    let nr = r + dr;
    let nc = c + dc;
    while (nr >= 0 && nr < 8 && nc >= 0 && nc < 8) {
      const p = board[nr * 8 + nc];
      if (p) {
        if (p.color === by && (p.kind === "b" || p.kind === "q")) return true;
        break;
      }
      nr += dr;
      nc += dc;
    }
  }

  return false;
}

function findKing(board: Array<ChessPiece | null>, color: ChessColor): number {
  for (let i = 0; i < 64; i++) {
    const p = board[i];
    if (p && p.color === color && p.kind === "k") return i;
  }
  return -1;
}

function inCheckBoard(board: Array<ChessPiece | null>, color: ChessColor): boolean {
  const k = findKing(board, color);
  return k >= 0 ? isSquareAttacked(board, k, enemy(color)) : false;
}

function applyBoardOnly(board: Array<ChessPiece | null>, m: ChessMove, turn: ChessColor): Array<ChessPiece | null> {
  const next = board.slice();
  const piece = next[m.from];
  if (!piece) return next;
  next[m.from] = null;
  if (m.enPassant) {
    const capSq = turn === "w" ? m.to + 8 : m.to - 8;
    next[capSq] = null;
  }
  if (m.castling === "k") {
    const row = turn === "w" ? 7 : 0;
    next[row * 8 + 5] = next[row * 8 + 7] ?? null;
    next[row * 8 + 7] = null;
  } else if (m.castling === "q") {
    const row = turn === "w" ? 7 : 0;
    next[row * 8 + 3] = next[row * 8 + 0] ?? null;
    next[row * 8 + 0] = null;
  }
  next[m.to] = m.promotion ? { color: turn, kind: m.promotion } : piece;
  return next;
}

function pseudoMoves(
  board: Array<ChessPiece | null>,
  turn: ChessColor,
  castling: CastlingRights,
  ep: number | null,
): ChessMove[] {
  const moves: ChessMove[] = [];
  const opp = enemy(turn);

  for (let sq = 0; sq < 64; sq++) {
    const p = board[sq];
    if (!p || p.color !== turn) continue;
    const r = Math.floor(sq / 8);
    const c = sq % 8;

    if (p.kind === "p") {
      const dir = turn === "w" ? -1 : 1;
      const startRow = turn === "w" ? 6 : 1;
      const promoRow = turn === "w" ? 0 : 7;
      const r1 = r + dir;
      if (r1 >= 0 && r1 < 8) {
        const fwd = r1 * 8 + c;
        if (!board[fwd]) {
          if (r1 === promoRow) {
            for (const pr of ["q", "r", "b", "n"] as PieceKind[]) {
              moves.push({ from: sq, to: fwd, promotion: pr });
            }
          } else {
            moves.push({ from: sq, to: fwd });
            if (r === startRow) {
              const fwd2 = (r + dir * 2) * 8 + c;
              if (!board[fwd2]) moves.push({ from: sq, to: fwd2 });
            }
          }
        }
        for (const dc of [-1, 1]) {
          const nc = c + dc;
          if (nc < 0 || nc >= 8) continue;
          const diag = r1 * 8 + nc;
          const target = board[diag];
          if (target && target.color === opp) {
            if (r1 === promoRow) {
              for (const pr of ["q", "r", "b", "n"] as PieceKind[]) {
                moves.push({ from: sq, to: diag, promotion: pr });
              }
            } else {
              moves.push({ from: sq, to: diag });
            }
          } else if (ep === diag) {
            moves.push({ from: sq, to: diag, enPassant: true });
          }
        }
      }
    } else if (p.kind === "n") {
      const steps: Array<[number, number]> = [
        [-2, -1], [-2, 1], [-1, -2], [-1, 2],
        [1, -2], [1, 2], [2, -1], [2, 1],
      ];
      for (const [dr, dc] of steps) {
        const nr = r + dr;
        const nc = c + dc;
        if (nr < 0 || nr >= 8 || nc < 0 || nc >= 8) continue;
        const to = nr * 8 + nc;
        const t = board[to];
        if (!t || t.color === opp) moves.push({ from: sq, to });
      }
    } else if (p.kind === "k") {
      for (let dr = -1; dr <= 1; dr++) {
        for (let dc = -1; dc <= 1; dc++) {
          if (dr === 0 && dc === 0) continue;
          const nr = r + dr;
          const nc = c + dc;
          if (nr < 0 || nr >= 8 || nc < 0 || nc >= 8) continue;
          const to = nr * 8 + nc;
          const t = board[to];
          if (!t || t.color === opp) moves.push({ from: sq, to });
        }
      }
      // Castling
      const homeRow = turn === "w" ? 7 : 0;
      if (r === homeRow && c === 4 && !isSquareAttacked(board, sq, opp)) {
        const canK = turn === "w" ? castling.wk : castling.bk;
        const canQ = turn === "w" ? castling.wq : castling.bq;
        if (
          canK &&
          !board[homeRow * 8 + 5] &&
          !board[homeRow * 8 + 6] &&
          board[homeRow * 8 + 7]?.kind === "r" &&
          board[homeRow * 8 + 7]?.color === turn &&
          !isSquareAttacked(board, homeRow * 8 + 5, opp) &&
          !isSquareAttacked(board, homeRow * 8 + 6, opp)
        ) {
          moves.push({ from: sq, to: homeRow * 8 + 6, castling: "k" });
        }
        if (
          canQ &&
          !board[homeRow * 8 + 1] &&
          !board[homeRow * 8 + 2] &&
          !board[homeRow * 8 + 3] &&
          board[homeRow * 8 + 0]?.kind === "r" &&
          board[homeRow * 8 + 0]?.color === turn &&
          !isSquareAttacked(board, homeRow * 8 + 3, opp) &&
          !isSquareAttacked(board, homeRow * 8 + 2, opp)
        ) {
          moves.push({ from: sq, to: homeRow * 8 + 2, castling: "q" });
        }
      }
    } else {
      const dirs: Array<[number, number]> =
        p.kind === "b"
          ? [[-1, -1], [-1, 1], [1, -1], [1, 1]]
          : p.kind === "r"
            ? [[-1, 0], [1, 0], [0, -1], [0, 1]]
            : [[-1, -1], [-1, 1], [1, -1], [1, 1], [-1, 0], [1, 0], [0, -1], [0, 1]];
      for (const [dr, dc] of dirs) {
        let nr = r + dr;
        let nc = c + dc;
        while (nr >= 0 && nr < 8 && nc >= 0 && nc < 8) {
          const to = nr * 8 + nc;
          const t = board[to];
          if (!t) {
            moves.push({ from: sq, to });
          } else {
            if (t.color === opp) moves.push({ from: sq, to });
            break;
          }
          nr += dr;
          nc += dc;
        }
      }
    }
  }
  return moves;
}

export function legalMoves(st: ChessState): ChessMove[] {
  if (st.status !== "playing") return [];
  const raw = pseudoMoves(st.board, st.turn, st.castling, st.ep);
  return raw.filter((m) => {
    const nextBoard = applyBoardOnly(st.board, m, st.turn);
    return !inCheckBoard(nextBoard, st.turn);
  });
}

function insufficientMaterial(board: Array<ChessPiece | null>): boolean {
  const pieces: Array<{ p: ChessPiece; sq: number }> = [];
  for (let i = 0; i < 64; i++) {
    const p = board[i];
    if (p && p.kind !== "k") pieces.push({ p, sq: i });
  }
  if (pieces.length === 0) return true;
  if (pieces.length === 1) {
    return pieces[0]!.p.kind === "n" || pieces[0]!.p.kind === "b";
  }
  if (pieces.length === 2 && pieces[0]!.p.kind === "b" && pieces[1]!.p.kind === "b") {
    const c0 = (Math.floor(pieces[0]!.sq / 8) + (pieces[0]!.sq % 8)) % 2;
    const c1 = (Math.floor(pieces[1]!.sq / 8) + (pieces[1]!.sq % 8)) % 2;
    return c0 === c1;
  }
  return false;
}

function formatRuMove(board: Array<ChessPiece | null>, m: ChessMove, check: boolean, mate: boolean): string {
  if (m.castling === "k") return `O-O${mate ? "#" : check ? "+" : ""}`;
  if (m.castling === "q") return `O-O-O${mate ? "#" : check ? "+" : ""}`;
  const piece = board[m.from];
  if (!piece) return moveToUci(m);
  const captured = m.enPassant || Boolean(board[m.to]);
  const toStr = sqToCoord(m.to);
  const fromFile = FILES[m.from % 8];
  const prefix = piece.kind === "p" ? (captured ? fromFile : "") : PIECE_RU_LETTER[piece.kind];
  const sep = captured ? ":" : "";
  const promo = m.promotion ? `=${PIECE_RU_LETTER[m.promotion]}` : "";
  const tail = mate ? "#" : check ? "+" : "";
  return `${prefix}${sep}${toStr}${promo}${tail}`;
}

function detectOpening(history: ChessHistoryItem[]): string | null {
  const line = history.map((h) => h.uci).join(" ");
  let found: string | null = null;
  let bestLen = 0;
  for (const [prefix, name] of OPENINGS) {
    if (line.startsWith(prefix) && prefix.length > bestLen) {
      bestLen = prefix.length;
      found = name;
    }
  }
  return found;
}

export function applyChessMove(st: ChessState, m: ChessMove): ChessState {
  const piece = st.board[m.from];
  if (!piece) return st;
  const target = m.enPassant ? { color: enemy(st.turn), kind: "p" as PieceKind } : st.board[m.to];
  const nextBoard = applyBoardOnly(st.board, m, st.turn);

  const castling: CastlingRights = { ...st.castling };
  if (piece.kind === "k") {
    if (st.turn === "w") {
      castling.wk = false;
      castling.wq = false;
    } else {
      castling.bk = false;
      castling.bq = false;
    }
  }
  if (m.from === 63 || m.to === 63) castling.wk = false;
  if (m.from === 56 || m.to === 56) castling.wq = false;
  if (m.from === 7 || m.to === 7) castling.bk = false;
  if (m.from === 0 || m.to === 0) castling.bq = false;

  const ep =
    piece.kind === "p" && Math.abs(m.to - m.from) === 16
      ? (m.from + m.to) / 2
      : null;

  const nextTurn = enemy(st.turn);
  const halfmove = piece.kind === "p" || target ? 0 : st.halfmove + 1;
  const fullmove = st.turn === "b" ? st.fullmove + 1 : st.fullmove;

  const key = posKey(nextBoard, nextTurn, castling, ep);
  const posCounts = { ...st.posCounts, [key]: (st.posCounts[key] ?? 0) + 1 };

  const capturedByWhite = st.capturedByWhite.slice();
  const capturedByBlack = st.capturedByBlack.slice();
  if (target) {
    if (st.turn === "w") capturedByWhite.push(target.kind);
    else capturedByBlack.push(target.kind);
  }

  const inCheck = inCheckBoard(nextBoard, nextTurn);
  const probe: ChessState = {
    board: nextBoard,
    turn: nextTurn,
    castling,
    ep,
    halfmove,
    fullmove,
    history: st.history,
    posCounts,
    capturedByWhite,
    capturedByBlack,
    openingName: st.openingName,
    status: "playing",
    winner: null,
    inCheck,
  };
  const nextMoves = legalMoves(probe);
  const checkmate = nextMoves.length === 0 && inCheck;
  const stalemate = nextMoves.length === 0 && !inCheck;

  let status: ChessState["status"] = "playing";
  let winner: ChessState["winner"] = null;
  if (checkmate) {
    status = "checkmate";
    winner = st.turn;
  } else if (stalemate) {
    status = "stalemate";
    winner = "draw";
  } else if ((posCounts[key] ?? 0) >= 3) {
    status = "draw_rep";
    winner = "draw";
  } else if (halfmove >= 100) {
    status = "draw_50";
    winner = "draw";
  } else if (insufficientMaterial(nextBoard)) {
    status = "draw_material";
    winner = "draw";
  }

  const uci = moveToUci(m);
  const ru = formatRuMove(st.board, m, inCheck, checkmate);
  const history: ChessHistoryItem[] = [
    ...st.history,
    {
      move: m,
      uci,
      ru,
      color: st.turn,
      captured: target?.kind,
      check: inCheck,
      checkmate,
    },
  ];
  const openingName = detectOpening(history) ?? st.openingName;

  return {
    ...probe,
    history,
    openingName,
    status,
    winner,
  };
}

// Static evaluation from White's perspective (centipawns)
export function evaluateChess(board: Array<ChessPiece | null>): number {
  let score = 0;
  let nonPawnMaterial = 0;
  let wBishops = 0;
  let bBishops = 0;

  for (let i = 0; i < 64; i++) {
    const p = board[i];
    if (p && p.kind !== "p" && p.kind !== "k") {
      nonPawnMaterial += PIECE_VALUE[p.kind];
    }
  }
  const endgame = nonPawnMaterial <= 2600;

  for (let sq = 0; sq < 64; sq++) {
    const p = board[sq];
    if (!p) continue;
    const pstSq = p.color === "w" ? sq : (7 - Math.floor(sq / 8)) * 8 + (sq % 8);
    let pst = 0;
    switch (p.kind) {
      case "p": pst = PST_PAWN[pstSq]!; break;
      case "n": pst = PST_KNIGHT[pstSq]!; break;
      case "b":
        pst = PST_BISHOP[pstSq]!;
        if (p.color === "w") wBishops++;
        else bBishops++;
        break;
      case "r": pst = PST_ROOK[pstSq]!; break;
      case "q": pst = PST_QUEEN[pstSq]!; break;
      case "k": pst = endgame ? PST_KING_END[pstSq]! : PST_KING_MID[pstSq]!; break;
    }
    const val = PIECE_VALUE[p.kind] + pst;
    score += p.color === "w" ? val : -val;
  }

  if (wBishops >= 2) score += 30;
  if (bBishops >= 2) score -= 30;
  return score;
}

export function materialBalanceSummary(st: ChessState): string {
  let w = 0;
  let b = 0;
  for (const p of st.board) {
    if (!p || p.kind === "k") continue;
    if (p.color === "w") w += PIECE_VALUE[p.kind];
    else b += PIECE_VALUE[p.kind];
  }
  const diff = Math.round((w - b) / 100);
  if (diff === 0) return "Материал поровну";
  return diff > 0 ? `У игрока +${diff} пеш.` : `У Джарвиса +${Math.abs(diff)} пеш.`;
}

function moveScore(board: Array<ChessPiece | null>, m: ChessMove): number {
  let s = 0;
  const victim = board[m.to];
  const attacker = board[m.from];
  if (victim && attacker) {
    s += 10 * PIECE_VALUE[victim.kind] - PIECE_VALUE[attacker.kind];
  } else if (m.enPassant) {
    s += 900;
  }
  if (m.promotion) s += PIECE_VALUE[m.promotion] * 8;
  if (m.castling) s += 150;
  return s;
}

interface SearchCtx {
  deadline: number;
  nodes: number;
}

function quiesce(
  board: Array<ChessPiece | null>,
  turn: ChessColor,
  castling: CastlingRights,
  ep: number | null,
  alpha: number,
  beta: number,
  qDepth: number,
  ctx: SearchCtx,
): number {
  ctx.nodes++;
  const standPat = evaluateChess(board) * (turn === "w" ? 1 : -1);
  if (standPat >= beta) return beta;
  if (standPat > alpha) alpha = standPat;
  if (qDepth <= 0 || performance.now() > ctx.deadline) return standPat;

  const caps = pseudoMoves(board, turn, castling, ep)
    .filter((m) => board[m.to] !== null || m.enPassant || m.promotion)
    .sort((a, b) => moveScore(board, b) - moveScore(board, a));

  for (const m of caps) {
    const nb = applyBoardOnly(board, m, turn);
    if (inCheckBoard(nb, turn)) continue;
    const score = -quiesce(nb, enemy(turn), castling, null, -beta, -alpha, qDepth - 1, ctx);
    if (score >= beta) return beta;
    if (score > alpha) alpha = score;
  }
  return alpha;
}

function negamax(
  board: Array<ChessPiece | null>,
  turn: ChessColor,
  castling: CastlingRights,
  ep: number | null,
  depth: number,
  alpha: number,
  beta: number,
  ply: number,
  ctx: SearchCtx,
): number {
  ctx.nodes++;
  if (depth <= 0) {
    return quiesce(board, turn, castling, ep, alpha, beta, 3, ctx);
  }
  if ((ctx.nodes & 255) === 0 && performance.now() > ctx.deadline) {
    return evaluateChess(board) * (turn === "w" ? 1 : -1);
  }

  const pseudo = pseudoMoves(board, turn, castling, ep);
  const legal: Array<{ m: ChessMove; nb: Array<ChessPiece | null>; order: number }> = [];
  for (const m of pseudo) {
    const nb = applyBoardOnly(board, m, turn);
    if (!inCheckBoard(nb, turn)) {
      legal.push({ m, nb, order: moveScore(board, m) });
    }
  }

  if (legal.length === 0) {
    if (inCheckBoard(board, turn)) return -30000 + ply;
    return 0;
  }

  legal.sort((a, b) => b.order - a.order);

  for (const { m, nb } of legal) {
    const nextEp = board[m.from]?.kind === "p" && Math.abs(m.to - m.from) === 16 ? (m.from + m.to) / 2 : null;
    const nextCast: CastlingRights = {
      wk: castling.wk && m.from !== 60 && m.from !== 63 && m.to !== 63,
      wq: castling.wq && m.from !== 60 && m.from !== 56 && m.to !== 56,
      bk: castling.bk && m.from !== 4 && m.from !== 7 && m.to !== 7,
      bq: castling.bq && m.from !== 4 && m.from !== 0 && m.to !== 0,
    };
    const score = -negamax(nb, enemy(turn), nextCast, nextEp, depth - 1, -beta, -alpha, ply + 1, ctx);
    if (score >= beta) return beta;
    if (score > alpha) alpha = score;
  }
  return alpha;
}

export function chooseChessMove(st: ChessState, difficulty: Difficulty): ChessMove | null {
  const moves = legalMoves(st);
  if (moves.length === 0) return null;
  if (moves.length === 1) return moves[0]!;

  // Opening book variety on medium/hard
  if (difficulty !== "easy" && st.history.length <= 6) {
    const line = st.history.map((h) => h.uci).join(" ");
    const bookChoices = BOOK[line];
    if (bookChoices && bookChoices.length > 0) {
      const pickUci = bookChoices[Math.floor(Math.random() * bookChoices.length)];
      const hit = moves.find((m) => moveToUci(m) === pickUci);
      if (hit) return hit;
    }
  }

  const maxDepth = difficulty === "easy" ? 2 : difficulty === "medium" ? 3 : 4;
  const budgetMs = difficulty === "easy" ? 90 : difficulty === "medium" ? 220 : 420;
  const ctx: SearchCtx = { deadline: performance.now() + budgetMs, nodes: 0 };

  const ordered = moves
    .slice()
    .sort((a, b) => moveScore(st.board, b) - moveScore(st.board, a));

  let bestMove = ordered[0]!;
  const scored: Array<{ m: ChessMove; score: number }> = [];

  for (let d = 1; d <= maxDepth; d++) {
    if (d > 1 && performance.now() > ctx.deadline) break;
    let curBest = ordered[0]!;
    let curBestScore = -Infinity;
    const passScores: Array<{ m: ChessMove; score: number }> = [];

    for (const m of ordered) {
      const next = applyChessMove(st, m);
      let s: number;
      if (next.status === "checkmate") {
        s = 30000;
      } else if (next.status !== "playing") {
        s = 0;
      } else {
        s = -negamax(next.board, next.turn, next.castling, next.ep, d - 1, -40000, 40000, 1, ctx);
      }
      passScores.push({ m, score: s });
      if (s > curBestScore) {
        curBestScore = s;
        curBest = m;
      }
    }
    bestMove = curBest;
    scored.splice(0, scored.length, ...passScores);
    scored.sort((a, b) => b.score - a.score);
  }

  if (difficulty === "easy" && scored.length >= 2) {
    // Pick playfully among top 3 reasonable moves unless there's mate-in-1
    if (scored[0]!.score < 20000 && Math.random() < 0.45) {
      const pool = scored.slice(0, Math.min(3, scored.length));
      return pool[Math.floor(Math.random() * pool.length)]!.m;
    }
  } else if (difficulty === "medium" && scored.length >= 2) {
    // Slight variety among near-equal moves (within 20 cp)
    const top = scored[0]!.score;
    const close = scored.filter((x) => top - x.score <= 20);
    if (close.length > 1) {
      return close[Math.floor(Math.random() * close.length)]!.m;
    }
  }

  return bestMove;
}

export function hintChessMove(st: ChessState): { move: ChessMove; text: string } | null {
  const m = chooseChessMove(st, "hard");
  if (!m) return null;
  const piece = st.board[m.from];
  const target = st.board[m.to];
  const next = applyChessMove(st, m);
  const from = sqToCoord(m.from);
  const to = sqToCoord(m.to);
  const pName = piece ? PIECE_RU_NAME[piece.kind] : "фигура";
  let reason = "укрепляет позицию и развивает фигуры";
  if (next.status === "checkmate") reason = "ставит мат в один ход!";
  else if (m.castling) reason = "прячет короля в безопасность и вводит ладью";
  else if (target) reason = `забирает фигуру соперника (${PIECE_RU_NAME[target.kind]})`;
  else if (next.inCheck) reason = "объявляет неприятный шах королю";
  else if (m.promotion) reason = "проводит пешку в ферзи";
  return {
    move: m,
    text: `Присмотрись к ходу ${pName} ${from}→${to} (${next.history.at(-1)?.ru ?? uciLabel(m)}) — он ${reason}.`,
  };
}

function uciLabel(m: ChessMove): string {
  return `${sqToCoord(m.from)}–${sqToCoord(m.to)}`;
}

/** Extract concise Russian tactical notes comparing state before & after moves. */
export function describeChessExchange(
  beforePlayer: ChessState,
  afterPlayer: ChessState,
  afterJarvis: ChessState | null,
): string[] {
  const notes: string[] = [];
  const pLast = afterPlayer.history.at(-1);
  if (pLast) {
    if (afterPlayer.openingName && afterPlayer.openingName !== beforePlayer.openingName) {
      notes.push(`На доске разыгран дебют: «${afterPlayer.openingName}»`);
    }
    if (pLast.checkmate) notes.push("Игрок поставил МАТ Джарвису!");
    else if (pLast.check) notes.push("Игрок объявил шах Джарвису");
    if (pLast.captured) notes.push(`Игрок забрал фигуру Джарвиса: ${PIECE_RU_NAME[pLast.captured]}`);
    if (pLast.move.castling) notes.push("Игрок сделал рокировку");
    if (pLast.move.promotion) notes.push("Игрок провёл пешку в ферзи!");
  }

  const evalBefore = evaluateChess(beforePlayer.board);
  const evalAfterP = evaluateChess(afterPlayer.board);
  // If player lost > 220cp on their own move (e.g. hung a piece that Jarvis then captured)
  if (afterJarvis) {
    const jLast = afterJarvis.history.at(-1);
    const evalAfterJ = evaluateChess(afterJarvis.board);
    if (evalBefore - evalAfterJ >= 260 && jLast?.captured) {
      notes.push(`Игрок зевнул материал — Джарвис забрал (${PIECE_RU_NAME[jLast.captured]}) и получил перевес`);
    } else if (jLast?.captured) {
      notes.push(`Джарвис побил фигуру игрока: ${PIECE_RU_NAME[jLast.captured]}`);
    }
    if (jLast?.checkmate) notes.push("Джарвис поставил МАТ игроку!");
    else if (jLast?.check) notes.push("Джарвис объявил шах королю игрока!");
    if (jLast?.move.castling) notes.push("Джарвис увёл короля в рокировку");
    if (jLast?.move.promotion) notes.push("Джарвис провёл пешку в ферзи!");
    if (afterJarvis.status.startsWith("draw") || afterJarvis.status === "stalemate") {
      notes.push("Партия завершилась вничью");
    }
  } else if (evalAfterP - evalBefore >= 260) {
    notes.push("Сильный удар игрока с большим материальным выигрышем");
  }

  return notes;
}

const RU_FILE_MAP: Record<string, string> = {
  а: "a", бэ: "b", бе: "b", б: "b", в: "b", цэ: "c", це: "c", с: "c", ц: "c", дэ: "d", де: "d", д: "d", е: "e", эф: "f", ф: "f", жэ: "g", же: "g", ж: "g", г: "g", аш: "h", х: "h", н: "h",
};
const RU_RANK_WORDS: Record<string, string> = {
  один: "1", раз: "1", два: "2", три: "3", четыре: "4", пять: "5", шесть: "6", семь: "7", восемь: "8",
};

/** Parse UCI (`e2e4`), SAN/Russian algebraic (`Nf3`, `Кf3`, `O-O`), or natural spoken Russian (`конь на f3`, `пешка е2 е4`). */
export function parseSpokenChessMove(st: ChessState, raw: string): ChessMove | null {
  const legal = legalMoves(st);
  if (legal.length === 0) return null;
  const s = raw.trim();
  const lower = s.toLowerCase().replace(/ё/g, "е");

  // Castling
  if (/^(o-o-o|0-0-0|длинная рокировка|рокировка в длинную)/i.test(lower)) {
    return legal.find((m) => m.castling === "q") ?? null;
  }
  if (/^(o-o|0-0|короткая рокировка|рокировка)/i.test(lower)) {
    return legal.find((m) => m.castling === "k") ?? null;
  }

  // Exact UCI e2e4 or e7e8q
  const uciClean = lower.replace(/[\s\-–—→:>]+/g, "");
  const uciMatch = uciClean.match(/^([a-h][1-8])([a-h][1-8])([qrbn])?$/);
  if (uciMatch) {
    const from = coordToSq(uciMatch[1]!);
    const to = coordToSq(uciMatch[2]!);
    const promo = (uciMatch[3] as PieceKind | undefined) ?? "q";
    return (
      legal.find((m) => m.from === from && m.to === to && (!m.promotion || m.promotion === promo)) ?? null
    );
  }

  // Normalize spoken Russian words ("конём на е четыре" -> piece hint + squares)
  let norm = lower;
  for (const [word, digit] of Object.entries(RU_RANK_WORDS)) {
    norm = norm.replace(new RegExp(`(^|[^a-zа-я0-9])(${word})(?=$|[^a-zа-я0-9])`, "gi"), `$1${digit}`);
  }
  // Convert Cyrillic file+digit pairs like "е4", "е 4", "эф 3", "аш 5"
  norm = norm.replace(
    /(аш|эф|бэ|бе|цэ|це|дэ|де|жэ|же|[абвсцдефжгхн])\s*([1-8])/g,
    (_, f: string, d: string) => `${RU_FILE_MAP[f] ?? f}${d}`,
  );

  let pieceFilter: PieceKind | null = null;
  if (/корол|кр/.test(lower)) pieceFilter = "k";
  else if (/ферз/.test(lower) || /^Q/.test(s) || /^Ф/.test(s)) pieceFilter = "q";
  else if (/ладь/.test(lower) || /^R/.test(s) || /^Л/.test(s)) pieceFilter = "r";
  else if (/слон/.test(lower) || /^B/.test(s) || /^С/.test(s)) pieceFilter = "b";
  else if (/кон/.test(lower) || /^N/.test(s) || /^К(?!р)/.test(s)) pieceFilter = "n";
  else if (/пешк/.test(lower)) pieceFilter = "p";

  const squares = [...norm.matchAll(/([a-h][1-8])/g)].map((m) => coordToSq(m[1]!)).filter((x): x is number => x !== null);
  if (squares.length >= 2) {
    const [from, to] = squares;
    return legal.find((m) => m.from === from && m.to === to && (!m.promotion || m.promotion === "q")) ?? null;
  }
  if (squares.length === 1) {
    const to = squares[0]!;
    const candidates = legal.filter((m) => {
      if (m.to !== to) return false;
      if (m.promotion && m.promotion !== "q") return false;
      const p = st.board[m.from];
      if (!p) return false;
      if (pieceFilter) return p.kind === pieceFilter;
      return true;
    });
    if (candidates.length === 1) return candidates[0]!;
    // If bare square like "e4", prefer pawn move
    if (!pieceFilter) {
      const pawnMoves = candidates.filter((m) => st.board[m.from]?.kind === "p");
      if (pawnMoves.length === 1) return pawnMoves[0]!;
    }
    if (candidates.length > 0) return candidates[0]!;
  }

  return null;
}
