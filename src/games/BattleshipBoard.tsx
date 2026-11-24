import { useEffect, useMemo, useState } from "react";
import {
  BS_COLS,
  bsCellToCoord,
  canPlaceShip,
  shipCells,
  type BattleshipState,
  type Ship,
  type ShotMark,
} from "./battleship";

interface Props {
  state: BattleshipState;
  disabled: boolean;
  hintCell: number | null;
  onShuffleFleet: () => void;
  onUpdateFleet: (ships: Ship[]) => void;
  onStartBattle: () => void;
  onFire: (cell: number) => void;
}

function FleetPips({ ships, label }: { ships: Ship[]; label: string }) {
  const sorted = useMemo(() => ships.slice().sort((a, b) => b.size - a.size || a.id - b.id), [ships]);
  return (
    <div className="gm-bs-fleet-bar" aria-label={label}>
      {sorted.map((s) => (
        <span key={s.id} className={`gm-bs-pip${s.sunk ? " is-sunk" : s.hits > 0 ? " is-damaged" : ""}`}>
          {Array.from({ length: s.size }, (_, k) => (
            <i key={k} />
          ))}
        </span>
      ))}
    </div>
  );
}

function ShotGlyph({ mark, revealShip }: { mark: ShotMark; revealShip?: boolean }) {
  if (mark === "sunk") {
    return (
      <span className="gm-bs-mark is-sunk" aria-hidden="true">
        <svg viewBox="0 0 16 16">
          <path d="M3.5 3.5l9 9M12.5 3.5l-9 9" />
        </svg>
      </span>
    );
  }
  if (mark === "hit") {
    return (
      <span className="gm-bs-mark is-hit" aria-hidden="true">
        <svg viewBox="0 0 16 16">
          <path d="M4 4l8 8M12 4l-8 8" />
        </svg>
      </span>
    );
  }
  if (mark === "miss") {
    return <span className="gm-bs-mark is-miss" aria-hidden="true" />;
  }
  if (mark === "halo") {
    return <span className="gm-bs-mark is-halo" aria-hidden="true" />;
  }
  if (revealShip) {
    return <span className="gm-bs-ghost-ship" aria-hidden="true" />;
  }
  return null;
}

export function BattleshipBoard({
  state,
  disabled,
  hintCell,
  onShuffleFleet,
  onUpdateFleet,
  onStartBattle,
  onFire,
}: Props) {
  const [movingShipId, setMovingShipId] = useState<number | null>(null);
  const [placeHoriz, setPlaceHoriz] = useState(true);
  const [hoverCell, setHoverCell] = useState<number | null>(null);

  const movingShip = useMemo(
    () => state.playerShips.find((s) => s.id === movingShipId) ?? null,
    [state.playerShips, movingShipId],
  );

  useEffect(() => {
    if (state.phase !== "setup") setMovingShipId(null);
  }, [state.phase]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (state.phase === "setup" && (e.key === "r" || e.key === "R" || e.key === "к" || e.key === "К")) {
        setPlaceHoriz((h) => !h);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [state.phase]);

  const playerShipMap = useMemo(() => {
    const m = new Map<number, Ship>();
    for (const s of state.playerShips) {
      for (const c of s.cells) m.set(c, s);
    }
    return m;
  }, [state.playerShips]);

  const jarvisShipSet = useMemo(() => {
    const s = new Set<number>();
    for (const sh of state.jarvisShips) {
      for (const c of sh.cells) s.add(c);
    }
    return s;
  }, [state.jarvisShips]);

  const previewCells = useMemo(() => {
    if (!movingShip || hoverCell === null) return { cells: [] as number[], valid: false };
    const r = Math.floor(hoverCell / 10);
    const c = hoverCell % 10;
    const cells = shipCells(r, c, movingShip.size, placeHoriz) ?? [];
    const valid =
      cells.length === movingShip.size &&
      canPlaceShip(state.playerShips, r, c, movingShip.size, placeHoriz, movingShip.id);
    return { cells, valid };
  }, [movingShip, hoverCell, placeHoriz, state.playerShips]);

  const lastPlayerShot = useMemo(() => {
    for (let i = state.history.length - 1; i >= 0; i--) {
      if (state.history[i]!.by === "player") return state.history[i]!.coord;
    }
    return null;
  }, [state.history]);

  const lastJarvisShot = useMemo(() => {
    for (let i = state.history.length - 1; i >= 0; i--) {
      if (state.history[i]!.by === "jarvis") return state.history[i]!.coord;
    }
    return null;
  }, [state.history]);

  const onSetupCellClick = (cell: number) => {
    if (state.phase !== "setup") return;
    if (movingShip) {
      const r = Math.floor(cell / 10);
      const c = cell % 10;
      if (canPlaceShip(state.playerShips, r, c, movingShip.size, placeHoriz, movingShip.id)) {
        const next = state.playerShips.map((s) =>
          s.id === movingShip.id
            ? { ...s, row: r, col: c, horiz: placeHoriz, cells: shipCells(r, c, s.size, placeHoriz)! }
            : s,
        );
        onUpdateFleet(next);
        setMovingShipId(null);
      }
      return;
    }
    const clickedShip = playerShipMap.get(cell);
    if (clickedShip) {
      setMovingShipId(clickedShip.id);
      setPlaceHoriz(clickedShip.horiz);
    }
  };

  if (state.phase === "setup") {
    return (
      <div className="gm-bs-setup">
        <div className="gm-bs-setup-head">
          <div>
            <h3>Расстановка вашего флота</h3>
            <p>
              Нажмите на любой корабль, чтобы переставить или повернуть его (клавиша <kbd>R</kbd>), либо начните бой сразу.
            </p>
          </div>
          <div className="gm-bs-setup-actions">
            {movingShip && (
              <button
                type="button"
                className="gm-btn"
                onClick={() => setPlaceHoriz((h) => !h)}
              >
                Повернуть ({placeHoriz ? "гориз." : "верт."})
              </button>
            )}
            <button type="button" className="gm-btn" onClick={onShuffleFleet}>
              Расставить случайно
            </button>
            <button type="button" className="gm-btn is-primary" onClick={onStartBattle}>
              В бой с Джарвисом!
            </button>
          </div>
        </div>

        <div className="gm-bs-grid-wrap is-setup">
          <div className="gm-bs-col-headers">
            <span />
            {BS_COLS.map((ch) => (
              <span key={ch}>{ch}</span>
            ))}
          </div>
          <div className="gm-bs-body">
            <div className="gm-bs-row-headers">
              {Array.from({ length: 10 }, (_, r) => (
                <span key={r}>{r + 1}</span>
              ))}
            </div>
            <div
              className="gm-bs-grid"
              onMouseLeave={() => setHoverCell(null)}
            >
              {Array.from({ length: 100 }, (_, cell) => {
                const ship = playerShipMap.get(cell);
                const isMoving = ship?.id === movingShipId;
                const inPreview = previewCells.cells.includes(cell);
                return (
                  <button
                    key={cell}
                    type="button"
                    aria-label={bsCellToCoord(cell)}
                    className={[
                      "gm-bs-cell",
                      ship && !isMoving ? "has-ship" : "",
                      isMoving ? "is-moving-origin" : "",
                      inPreview ? (previewCells.valid ? "is-preview-ok" : "is-preview-bad") : "",
                    ]
                      .filter(Boolean)
                      .join(" ")}
                    onMouseEnter={() => setHoverCell(cell)}
                    onClick={() => onSetupCellClick(cell)}
                  />
                );
              })}
            </div>
          </div>
        </div>
      </div>
    );
  }

  const finished = state.phase === "finished" || state.phase === "resigned";

  return (
    <div className="gm-bs-battle">
      <div className="gm-bs-boards">
        {/* Attack Radar: Jarvis's ocean */}
        <div className="gm-bs-pane is-radar">
          <div className="gm-bs-pane-head">
            <span className="gm-dot is-jarvis" />
            <b>Акватория Джарвиса · Ваша цель</b>
            <span className="gm-count-pill">
              На плаву: {state.jarvisShips.filter((s) => !s.sunk).length}/10
            </span>
          </div>

          <div className="gm-bs-grid-wrap">
            <div className="gm-bs-col-headers">
              <span />
              {BS_COLS.map((ch) => (
                <span key={ch}>{ch}</span>
              ))}
            </div>
            <div className="gm-bs-body">
              <div className="gm-bs-row-headers">
                {Array.from({ length: 10 }, (_, r) => (
                  <span key={r}>{r + 1}</span>
                ))}
              </div>
              <div className="gm-bs-grid is-target" role="grid" aria-label="Акватория Джарвиса">
                {Array.from({ length: 100 }, (_, cell) => {
                  const mark = state.playerShots[cell]!;
                  const coord = bsCellToCoord(cell);
                  const canShoot = !disabled && state.phase === "playing" && state.turn === "player" && mark === "unknown";
                  const isLast = lastPlayerShot === coord;
                  const isHint = hintCell === cell;
                  const reveal = finished && mark === "unknown" && jarvisShipSet.has(cell);

                  return (
                    <button
                      key={cell}
                      type="button"
                      disabled={!canShoot}
                      aria-label={`Сектор ${coord}`}
                      title={coord}
                      className={[
                        "gm-bs-cell",
                        `mark-${mark}`,
                        canShoot ? "can-shoot" : "",
                        isLast ? "is-last" : "",
                        isHint ? "is-hint" : "",
                      ]
                        .filter(Boolean)
                        .join(" ")}
                      onClick={() => canShoot && onFire(cell)}
                    >
                      <ShotGlyph mark={mark} revealShip={reveal} />
                    </button>
                  );
                })}
              </div>
            </div>
          </div>

          <FleetPips ships={state.jarvisShips} label="Корабли Джарвиса" />
        </div>

        {/* Defense Grid: Player's fleet */}
        <div className="gm-bs-pane is-defense">
          <div className="gm-bs-pane-head">
            <span className="gm-dot is-user" />
            <b>Ваш флот</b>
            <span className="gm-count-pill">
              На плаву: {state.playerShips.filter((s) => !s.sunk).length}/10
            </span>
          </div>

          <div className="gm-bs-grid-wrap">
            <div className="gm-bs-col-headers">
              <span />
              {BS_COLS.map((ch) => (
                <span key={ch}>{ch}</span>
              ))}
            </div>
            <div className="gm-bs-body">
              <div className="gm-bs-row-headers">
                {Array.from({ length: 10 }, (_, r) => (
                  <span key={r}>{r + 1}</span>
                ))}
              </div>
              <div className="gm-bs-grid is-own" role="grid" aria-label="Ваш флот">
                {Array.from({ length: 100 }, (_, cell) => {
                  const mark = state.jarvisShots[cell]!;
                  const hasShip = playerShipMap.has(cell);
                  const coord = bsCellToCoord(cell);
                  const isLast = lastJarvisShot === coord;
                  return (
                    <div
                      key={cell}
                      className={[
                        "gm-bs-cell",
                        hasShip ? "has-ship" : "",
                        `mark-${mark}`,
                        isLast ? "is-last" : "",
                      ]
                        .filter(Boolean)
                        .join(" ")}
                    >
                      <ShotGlyph mark={mark} />
                    </div>
                  );
                })}
              </div>
            </div>
          </div>

          <FleetPips ships={state.playerShips} label="Ваши корабли" />
        </div>
      </div>
    </div>
  );
}
