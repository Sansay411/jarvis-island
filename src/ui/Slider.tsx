// Compact horizontal level control used for volume and brightness.
// The fill tracks the pointer immediately; the native setter is throttled
// while dragging and committed on release, then the verified value read back
// from Windows replaces the optimistic one.

import { useEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";
import { Icon, type IconName } from "./Icon";
import "./slider.css";

interface SliderProps {
  icon: IconName;
  label: string;
  value: number | null;
  disabled?: boolean;
  disabledReason?: string;
  throttleMs?: number;
  onChange: (v: number) => void;
  onIconClick?: () => void;
  iconActive?: boolean;
}

export function Slider({ icon, label, value, disabled, disabledReason, throttleMs = 60, onChange, onIconClick, iconActive }: SliderProps) {
  const track = useRef<HTMLDivElement>(null);
  const [local, setLocal] = useState<number | null>(null);
  const last = useRef(0);
  const pending = useRef<number | null>(null);
  const flush = useRef<number | undefined>(undefined);

  const shown = local ?? value ?? 0;

  useEffect(() => () => window.clearTimeout(flush.current), []);

  const emit = (v: number, force = false) => {
    const now = performance.now();
    pending.current = v;
    if (force || now - last.current >= throttleMs) {
      last.current = now;
      pending.current = null;
      onChange(v);
    } else {
      window.clearTimeout(flush.current);
      flush.current = window.setTimeout(() => {
        if (pending.current !== null) {
          last.current = performance.now();
          onChange(pending.current);
          pending.current = null;
        }
      }, throttleMs);
    }
  };

  const valueAt = (clientX: number) => {
    const r = track.current!.getBoundingClientRect();
    return Math.round(Math.min(1, Math.max(0, (clientX - r.left) / r.width)) * 100);
  };

  const onDown = (e: PointerEvent<HTMLDivElement>) => {
    if (disabled) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    const v = valueAt(e.clientX);
    setLocal(v);
    emit(v);
  };
  const onMove = (e: PointerEvent<HTMLDivElement>) => {
    if (disabled || !e.currentTarget.hasPointerCapture(e.pointerId)) return;
    const v = valueAt(e.clientX);
    setLocal(v);
    emit(v);
  };
  const onUp = (e: PointerEvent<HTMLDivElement>) => {
    if (disabled) return;
    const v = valueAt(e.clientX);
    emit(v, true);
    // Keep the optimistic value until the verified one arrives.
    window.setTimeout(() => setLocal(null), 250);
  };
  const onKey = (e: KeyboardEvent<HTMLDivElement>) => {
    if (disabled) return;
    const step = e.shiftKey ? 10 : 2;
    if (e.key === "ArrowRight" || e.key === "ArrowUp") emit(Math.min(100, shown + step), true);
    else if (e.key === "ArrowLeft" || e.key === "ArrowDown") emit(Math.max(0, shown - step), true);
    else return;
    e.preventDefault();
  };

  return (
    <div className={`slider${disabled ? " is-disabled" : ""}`} title={disabled ? disabledReason : undefined}>
      <button
        className={`slider-icon${iconActive ? " is-active" : ""}`}
        onClick={onIconClick}
        disabled={!onIconClick || disabled}
        aria-label={label}
      >
        <Icon name={icon} size={16} />
      </button>
      <div
        ref={track}
        className="slider-track"
        role="slider"
        tabIndex={disabled ? -1 : 0}
        aria-label={label}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={shown}
        aria-disabled={disabled}
        onPointerDown={onDown}
        onPointerMove={onMove}
        onPointerUp={onUp}
        onKeyDown={onKey}
      >
        <div className="slider-fill" style={{ transform: `scaleX(${shown / 100})` }} />
      </div>
      <span className="slider-value">{value === null && local === null ? "—" : `${shown}`}</span>
    </div>
  );
}
