import { useMemo, useState, type DragEvent } from "react";
import {
  legalMoves,
  PIECE_RU_NAME,
  PIECE_VALUE,
  sqToCoord,
  type ChessColor,
  type ChessMove,
  type ChessPiece,
  type ChessState,
  type PieceKind,
} from "./chess";

const FILES = ["a", "b", "c", "d", "e", "f", "g", "h"];
const RANKS = ["8", "7", "6", "5", "4", "3", "2", "1"];

function PieceSvg({ piece, size = 48 }: { piece: ChessPiece; size?: number }) {
  const isW = piece.color === "w";
  const fill = isW ? "url(#chess-ivory)" : "url(#chess-obsidian)";
  const stroke = isW ? "#22232a" : "#ffb23f";
  const strokeW = isW ? 1.5 : 1.35;
  const detail = isW ? "#2b2c34" : "#ffd280";

  return (
    <svg className={`gm-piece is-${piece.color}`} width={size} height={size} viewBox="0 0 45 45" aria-hidden="true">
      <defs>
        <linearGradient id="chess-ivory" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0%" stopColor="#fffdf9" />
          <stop offset="60%" stopColor="#eae7de" />
          <stop offset="100%" stopColor="#cfcabd" />
        </linearGradient>
        <linearGradient id="chess-obsidian" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0%" stopColor="#2c2e39" />
          <stop offset="55%" stopColor="#181920" />
          <stop offset="100%" stopColor="#0d0e13" />
        </linearGradient>
      </defs>
      <g fill={fill} stroke={stroke} strokeWidth={strokeW} strokeLinecap="round" strokeLinejoin="round">
        {piece.kind === "p" && (
          <path d="M22.5 9c-2.21 0-4 1.79-4 4 0 .89.29 1.71.78 2.38C17.33 16.5 16 18.59 16 21c0 2.03.94 3.84 2.41 5.03-3 1.06-7.41 5.55-7.41 13.47h23c0-7.92-4.41-12.41-7.41-13.47 1.47-1.19 2.41-3 2.41-5.03 0-2.41-1.33-4.5-3.28-5.62.49-.67.78-1.49.78-2.38 0-2.21-1.79-4-4-4z" />
        )}
        {piece.kind === "r" && (
          <>
            <path d="M9 39h27v-3H9v3zM12 36v-4h21v4H12zM11 14V9h4v2h5V9h5v2h5V9h4v5" />
            <path d="M34 14l-3 3H14l-3-3" />
            <path d="M31 17v12.5H14V17" />
            <path d="M31 29.5l1.5 2.5h-20l1.5-2.5" />
            <path d="M11 14h23" fill="none" stroke={detail} strokeWidth={1.1} />
          </>
        )}
        {piece.kind === "n" && (
          <>
            <path d="M22 10c10.5 1 16.5 8 16 29H15c0-9 10-6.5 8-21" />
            <path d="M24 18c.38 2.91-5.55 7.37-8 9-3 2-2.82 4.34-5 4-1.04-.94 1.41-3.04 0-3-1 0 .19 1.23-1 2-1 0-4 1-4-4 0-2 6-12 6-12s1.89-1.9 2-3.5c-.73-.99-.5-2-.5-3 1-1 3 2.5 3 2.5h2s.78-1.99 2.5-3c1 0 1 3 1 3" />
            <circle cx="15.5" cy="15.5" r="1.1" fill={detail} stroke="none" />
          </>
        )}
        {piece.kind === "b" && (
          <>
            <path d="M9 36c3.39-.97 10.11.43 13.5-2 3.39 2.43 10.11 1.03 13.5 2 0 0 1.65.54 3 2-.68.97-1.65.99-3 .5-3.39-.97-10.11.46-13.5-1-3.39 1.46-10.11.03-13.5 1-1.35.49-2.32.47-3-.5 1.35-1.46 3-2 3-2z" />
            <path d="M15 32c2.5 2.5 12.5 2.5 15 0 .5-1.5 0-2 0-2 0-2.5-2.5-4-2.5-4 5.5-1.5 6-11.5-5-15.5-11 4-10.5 14-5 15.5 0 0-2.5 1.5-2.5 4 0 0-.5.5 0 2z" />
            <circle cx="22.5" cy="8" r="2.3" />
            <path d="M17.5 26h10M15 30h15M22.5 15.5v5M20 18h5" fill="none" stroke={detail} strokeWidth={1.2} />
          </>
        )}
        {piece.kind === "q" && (
          <>
            <path d="M8 12a2 2 0 1 1-4 0 2 2 0 0 1 4 0zM24.5 7.5a2 2 0 1 1-4 0 2 2 0 0 1 4 0zM41 12a2 2 0 1 1-4 0 2 2 0 0 1 4 0zM16 8.5a2 2 0 1 1-4 0 2 2 0 0 1 4 0zM33 9a2 2 0 1 1-4 0 2 2 0 0 1 4 0z" />
            <path d="M9 26c8.5-1.5 21-1.5 27 0l2-12-7 11V11l-5.5 13.5-3-15-3 15-5.5-14V25L7 14l2 12z" />
            <path d="M9 26c0 2 1.5 2 2.5 4 1 1.5 1 1 .5 3.5-1.5 1-1.5 2.5-1.5 2.5-1.5 1.5.5 2.5.5 2.5 6.5 1 16.5 1 23 0 0 0 1.5-1 0-2.5 0 0 .5-1.5-1-2.5-.5-2.5-.5-2 .5-3.5 1-2 2.5-2 2.5-4-8.5-1.5-18.5-1.5-27 0z" />
            <path d="M11.5 30c3.5-1 18.5-1 22 0M12 33.5c6-1 15-1 21 0" fill="none" stroke={detail} strokeWidth={1.1} />
          </>
        )}
        {piece.kind === "k" && (
          <>
            <path d="M22.5 11.63V6M20 8h5" fill="none" stroke={stroke} strokeWidth={1.8} />
            <path d="M22.5 25s4.5-7.5 3-10.5c0 0-1-2.5-3-2.5s-3 2.5-3 2.5c-1.5 3 3 10.5 3 10.5" />
            <path d="M11.5 37c5.5 3.5 15.5 3.5 21 0v-7s9-4.5 6-10.5c-4-6.5-13.5-3.5-16 4V27v-3.5c-3.5-7.5-13-10.5-16-4-3 6 5 10 5 10V37z" />
            <path d="M11.5 30c5.5-3 15.5-3 21 0M11.5 33.5c5.5-3 15.5-3 21 0M11.5 37c5.5-3 15.5-3 21 0" fill="none" stroke={detail} strokeWidth={1.1} />
          </>
        )}
      </g>
    </svg>
  );
}

interface Props {
  state: ChessState;
  flipped: boolean;
  disabled: boolean;
  hintMove: ChessMove | null;
  onMove: (m: ChessMove) => void;
}

export function ChessBoard({ state, flipped, disabled, hintMove, onMove }: Props) {
  const [selected, setSelected] = useState<number | null>(null);
  const [promoPending, setPromoPending] = useState<{ from: number; to: number } | null>(null);

  const moves = useMemo(() => legalMoves(state), [state]);
  const lastMove = state.history.at(-1)?.move ?? null;

  const targetsFromSelected = useMemo(() => {
    if (selected === null) return new Map<number, ChessMove[]>();
    const map = new Map<number, ChessMove[]>();
    for (const m of moves) {
      if (m.from !== selected) continue;
      const list = map.get(m.to) ?? [];
      list.push(m);
      map.set(m.to, list);
    }
    return map;
  }, [moves, selected]);

  const kingCheckSq = useMemo(() => {
    if (!state.inCheck) return -1;
    return state.board.findIndex((p) => p && p.color === state.turn && p.kind === "k");
  }, [state]);

  const handleSquareClick = (sq: number) => {
    if (disabled || state.status !== "playing" || state.turn !== "w") return;
    const hitMoves = targetsFromSelected.get(sq);
    if (selected !== null && hitMoves && hitMoves.length > 0) {
      if (hitMoves.some((m) => m.promotion)) {
        setPromoPending({ from: selected, to: sq });
      } else {
        setSelected(null);
        onMove(hitMoves[0]!);
      }
      return;
    }
    const p = state.board[sq];
    if (p && p.color === "w") {
      setSelected(sq === selected ? null : sq);
    } else {
      setSelected(null);
    }
  };

  const onDragStart = (e: DragEvent, sq: number) => {
    if (disabled || state.status !== "playing" || state.turn !== "w") {
      e.preventDefault();
      return;
    }
    const p = state.board[sq];
    if (!p || p.color !== "w") {
      e.preventDefault();
      return;
    }
    setSelected(sq);
    e.dataTransfer.setData("text/plain", String(sq));
    e.dataTransfer.effectAllowed = "move";
  };

  const onDrop = (e: DragEvent, toSq: number) => {
    e.preventDefault();
    const raw = e.dataTransfer.getData("text/plain");
    const fromSq = raw ? Number(raw) : selected;
    if (fromSq === null || Number.isNaN(fromSq)) return;
    const match = moves.filter((m) => m.from === fromSq && m.to === toSq);
    if (match.length === 0) return;
    if (match.some((m) => m.promotion)) {
      setPromoPending({ from: fromSq, to: toSq });
    } else {
      setSelected(null);
      onMove(match[0]!);
    }
  };

  const indices = useMemo(() => {
    const arr = Array.from({ length: 64 }, (_, i) => i);
    return flipped ? arr.reverse() : arr;
  }, [flipped]);

  const sortCaptured = (list: PieceKind[], color: ChessColor) =>
    list
      .slice()
      .sort((a, b) => PIECE_VALUE[b] - PIECE_VALUE[a])
      .map((kind, i) => <PieceSvg key={`${kind}-${i}`} piece={{ color, kind }} size={20} />);

  const matDiff = useMemo(() => {
    let w = 0, b = 0;
    for (const p of state.board) {
      if (!p || p.kind === "k") continue;
      if (p.color === "w") w += PIECE_VALUE[p.kind];
      else b += PIECE_VALUE[p.kind];
    }
    return Math.round((w - b) / 100);
  }, [state.board]);

  return (
    <div className="gm-board-shell">
      <div className="gm-player-strip is-opponent">
        <div className="gm-strip-who">
          <span className="gm-dot is-jarvis" />
          <b>Джарвис · Чёрные</b>
          {state.openingName && <span className="gm-opening-pill">{state.openingName}</span>}
        </div>
        <div className="gm-captured-row">
          {sortCaptured(state.capturedByBlack, "w")}
          {matDiff < 0 && <span className="gm-mat-badge">+{Math.abs(matDiff)}</span>}
        </div>
      </div>

      <div className="gm-chess-frame">
        <div className="gm-chess-grid" role="grid" aria-label="Шахматная доска">
          {indices.map((sq, visualIdx) => {
            const r = Math.floor(sq / 8);
            const c = sq % 8;
            const dark = (r + c) % 2 === 1;
            const piece = state.board[sq];
            const isSel = selected === sq;
            const isLast = lastMove?.from === sq || lastMove?.to === sq;
            const isTarget = targetsFromSelected.has(sq);
            const isCapture = isTarget && (piece !== null || state.ep === sq);
            const isHint = hintMove?.from === sq || hintMove?.to === sq;
            const isCheck = kingCheckSq === sq;
            const showRank = visualIdx % 8 === 0;
            const showFile = visualIdx >= 56;

            return (
              <button
                key={sq}
                type="button"
                role="gridcell"
                aria-label={`${sqToCoord(sq)}${piece ? ` ${PIECE_RU_NAME[piece.kind]}` : ""}`}
                className={[
                  "gm-sq",
                  dark ? "is-dark" : "is-light",
                  isSel ? "is-selected" : "",
                  isLast ? "is-last" : "",
                  isHint ? "is-hint" : "",
                  isCheck ? "is-check" : "",
                ]
                  .filter(Boolean)
                  .join(" ")}
                onClick={() => handleSquareClick(sq)}
                onDragOver={(e) => e.preventDefault()}
                onDrop={(e) => onDrop(e, sq)}
              >
                {showRank && <span className="gm-coord gm-coord-rank">{RANKS[r]}</span>}
                {showFile && <span className="gm-coord gm-coord-file">{FILES[c]}</span>}
                {isTarget && !isCapture && <span className="gm-move-dot" />}
                {isCapture && <span className="gm-capture-ring" />}
                {piece && (
                  <span
                    className="gm-piece-wrap"
                    draggable={!disabled && piece.color === "w" && state.turn === "w"}
                    onDragStart={(e) => onDragStart(e, sq)}
                  >
                    <PieceSvg piece={piece} size={50} />
                  </span>
                )}
              </button>
            );
          })}
        </div>

        {promoPending && (
          <div className="gm-promo-overlay" onClick={() => setPromoPending(null)}>
            <div className="gm-promo-card" onClick={(e) => e.stopPropagation()}>
              <span>Превратить пешку:</span>
              <div className="gm-promo-choices">
                {(["q", "r", "b", "n"] as PieceKind[]).map((k) => (
                  <button
                    key={k}
                    type="button"
                    className="gm-promo-btn"
                    title={PIECE_RU_NAME[k]}
                    onClick={() => {
                      const mv = moves.find(
                        (m) => m.from === promoPending.from && m.to === promoPending.to && m.promotion === k,
                      );
                      setPromoPending(null);
                      setSelected(null);
                      if (mv) onMove(mv);
                    }}
                  >
                    <PieceSvg piece={{ color: "w", kind: k }} size={42} />
                    <small>{PIECE_RU_NAME[k]}</small>
                  </button>
                ))}
              </div>
            </div>
          </div>
        )}
      </div>

      <div className="gm-player-strip is-user">
        <div className="gm-strip-who">
          <span className="gm-dot is-user" />
          <b>Вы · Белые</b>
          {state.inCheck && state.turn === "w" && <span className="gm-alert-pill">Шах!</span>}
        </div>
        <div className="gm-captured-row">
          {sortCaptured(state.capturedByWhite, "b")}
          {matDiff > 0 && <span className="gm-mat-badge">+{matDiff}</span>}
        </div>
      </div>
    </div>
  );
}
