import { useMemo, useState } from "react";
import {
  checkersScoreSummary,
  ckSqToCoord,
  legalCheckersMoves,
  type CheckerPiece,
  type CheckersMove,
  type CheckersState,
} from "./checkers";

const FILES = ["a", "b", "c", "d", "e", "f", "g", "h"];
const RANKS = ["8", "7", "6", "5", "4", "3", "2", "1"];

function CheckerDisc({ piece }: { piece: CheckerPiece }) {
  return (
    <span className={`gm-checker is-${piece.color}${piece.king ? " is-king" : ""}`}>
      <span className="gm-checker-core">
        {piece.king && (
          <svg viewBox="0 0 24 24" className="gm-checker-crown" aria-hidden="true">
            <path d="M4 17l1.5-9 6.5 4.5L18.5 8 20 17H4zm1 2.5h14" />
          </svg>
        )}
      </span>
    </span>
  );
}

interface Props {
  state: CheckersState;
  flipped: boolean;
  disabled: boolean;
  hintMove: CheckersMove | null;
  onMove: (m: CheckersMove) => void;
}

export function CheckersBoard({ state, flipped, disabled, hintMove, onMove }: Props) {
  const [selected, setSelected] = useState<number | null>(null);

  const moves = useMemo(() => legalCheckersMoves(state), [state]);
  const mustCapture = moves.length > 0 && moves[0]!.captures.length > 0;
  const movableOrigins = useMemo(() => new Set(moves.map((m) => m.from)), [moves]);
  const lastMove = state.history.at(-1)?.move ?? null;

  const selectedMoves = useMemo(
    () => (selected === null ? [] : moves.filter((m) => m.from === selected)),
    [moves, selected],
  );

  const targets = useMemo(() => {
    const map = new Map<number, CheckersMove>();
    for (const m of selectedMoves) {
      map.set(m.to, m);
    }
    return map;
  }, [selectedMoves]);

  const viaSquares = useMemo(() => {
    const s = new Set<number>();
    for (const m of selectedMoves) {
      for (let i = 0; i < m.path.length - 1; i++) s.add(m.path[i]!);
    }
    return s;
  }, [selectedMoves]);

  const summary = useMemo(() => checkersScoreSummary(state), [state]);

  const handleClick = (sq: number) => {
    if (disabled || state.status !== "playing" || state.turn !== "w") return;
    const hit = targets.get(sq);
    if (selected !== null && hit) {
      setSelected(null);
      onMove(hit);
      return;
    }
    const p = state.board[sq];
    if (p && p.color === "w" && movableOrigins.has(sq)) {
      setSelected(sq === selected ? null : sq);
    } else {
      setSelected(null);
    }
  };

  const indices = useMemo(() => {
    const arr = Array.from({ length: 64 }, (_, i) => i);
    return flipped ? arr.reverse() : arr;
  }, [flipped]);

  return (
    <div className="gm-board-shell">
      <div className="gm-player-strip is-opponent">
        <div className="gm-strip-who">
          <span className="gm-dot is-jarvis" />
          <b>Джарвис · Чёрные</b>
        </div>
        <div className="gm-captured-row">
          <span className="gm-count-pill">
            Шашек: {summary.bMen + summary.bKings}
            {summary.bKings > 0 ? ` · дамок ${summary.bKings}` : ""}
          </span>
        </div>
      </div>

      <div className="gm-chess-frame">
        <div className="gm-chess-grid" role="grid" aria-label="Доска для русских шашек">
          {indices.map((sq, visualIdx) => {
            const r = Math.floor(sq / 8);
            const c = sq % 8;
            const dark = (r + c) % 2 === 1;
            const piece = state.board[sq];
            const isSel = selected === sq;
            const isLast = lastMove?.from === sq || lastMove?.to === sq;
            const targetMove = targets.get(sq);
            const isVia = viaSquares.has(sq);
            const isHint = hintMove?.from === sq || hintMove?.to === sq;
            const canPick =
              !disabled && state.turn === "w" && piece?.color === "w" && movableOrigins.has(sq);
            const showRank = visualIdx % 8 === 0;
            const showFile = visualIdx >= 56;

            return (
              <button
                key={sq}
                type="button"
                role="gridcell"
                aria-label={ckSqToCoord(sq)}
                className={[
                  "gm-sq",
                  dark ? "is-dark" : "is-light",
                  isSel ? "is-selected" : "",
                  isLast ? "is-last" : "",
                  isHint ? "is-hint" : "",
                  mustCapture && canPick ? "is-must-capture" : "",
                ]
                  .filter(Boolean)
                  .join(" ")}
                onClick={() => handleClick(sq)}
              >
                {showRank && <span className="gm-coord gm-coord-rank">{RANKS[r]}</span>}
                {showFile && <span className="gm-coord gm-coord-file">{FILES[c]}</span>}
                {isVia && !targetMove && <span className="gm-via-dot" />}
                {targetMove && targetMove.captures.length === 0 && <span className="gm-move-dot" />}
                {targetMove && targetMove.captures.length > 0 && <span className="gm-capture-ring" />}
                {piece && <CheckerDisc piece={piece} />}
              </button>
            );
          })}
        </div>
      </div>

      <div className="gm-player-strip is-user">
        <div className="gm-strip-who">
          <span className="gm-dot is-user" />
          <b>Вы · Белые</b>
          {mustCapture && state.turn === "w" && (
            <span className="gm-alert-pill is-amber">Обязательный бой!</span>
          )}
        </div>
        <div className="gm-captured-row">
          <span className="gm-count-pill">
            Шашек: {summary.wMen + summary.wKings}
            {summary.wKings > 0 ? ` · дамок ${summary.wKings}` : ""}
          </span>
        </div>
      </div>
    </div>
  );
}
