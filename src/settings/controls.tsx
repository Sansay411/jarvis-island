// Building blocks shared by the settings sections.

import { motion } from "motion/react";
import type { ReactNode } from "react";

export function Row({ label, hint, children, wide }: { label: ReactNode; hint?: ReactNode; children: ReactNode; wide?: boolean }) {
  return (
    <div className={`row${wide ? " is-wide" : ""}`}>
      <div className="row-text">
        <div className="row-label">{label}</div>
        {hint && <div className="row-hint">{hint}</div>}
      </div>
      <div className="row-control">{children}</div>
    </div>
  );
}

export function Group({ title, children }: { title?: string; children: ReactNode }) {
  return (
    <div className="group">
      {title && <h2>{title}</h2>}
      <div className="group-rows">{children}</div>
    </div>
  );
}

export function Switch({ checked, onChange, label }: { checked: boolean; onChange: (v: boolean) => void; label: string }) {
  return (
    <button role="switch" aria-checked={checked} aria-label={label} className={`switch${checked ? " is-on" : ""}`} onClick={() => onChange(!checked)}>
      <span />
    </button>
  );
}

export function Segmented<T extends string>({ value, options, onChange, label }: { value: T; options: { id: T; label: string }[]; onChange: (v: T) => void; label: string }) {
  return (
    <div className="segmented" role="radiogroup" aria-label={label}>
      {options.map((o) => (
        <button key={o.id} role="radio" aria-checked={value === o.id} className={value === o.id ? "is-on" : ""} onClick={() => onChange(o.id)}>
          {value === o.id && <motion.span layoutId={`seg-${label}`} className="segmented-thumb" transition={{ type: "spring", stiffness: 500, damping: 38 }} />}
          <span className="segmented-text">{o.label}</span>
        </button>
      ))}
    </div>
  );
}
