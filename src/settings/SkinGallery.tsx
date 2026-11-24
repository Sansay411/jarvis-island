// Settings → Характер → Облик: live animated preview cards for all five skins
// with a «Показать эмоции» mood-cycle button.

import { useMotionValue, useReducedMotion } from "motion/react";
import { useEffect, useState } from "react";
import { JarvisFace } from "../island/skins/JarvisFace";
import { SKINS, type EyeMood, type SkinId } from "../island/skins/types";

const DEMO_CYCLE: { mood: EyeMood; label: string }[] = [
  { mood: "thinking", label: "Думает" },
  { mood: "listening", label: "Слушает" },
  { mood: "speaking", label: "Говорит" },
  { mood: "happy", label: "Радость" },
  { mood: "error", label: "Ошибка" },
  { mood: "sleep", label: "Сон" },
  { mood: "attentive", label: "Внимание" },
];

export function SkinGallery({
  value,
  onChange,
  reduceMotion = false,
}: {
  value: SkinId;
  onChange: (skin: SkinId) => void;
  reduceMotion?: boolean;
}) {
  const osReduced = useReducedMotion();
  const reduced = Boolean(osReduced || reduceMotion);
  const [cycling, setCycling] = useState(false);
  const [step, setStep] = useState(0);
  const demoLevel = useMotionValue(0);

  const activeDemo = cycling ? DEMO_CYCLE[step % DEMO_CYCLE.length]! : null;
  const previewMood: EyeMood = activeDemo ? activeDemo.mood : "attentive";

  useEffect(() => {
    if (!cycling) {
      setStep(0);
      return;
    }
    const id = window.setInterval(() => {
      setStep((s) => (s + 1) % DEMO_CYCLE.length);
    }, 1500);
    return () => window.clearInterval(id);
  }, [cycling]);

  // Simulate realistic voice levels while cycling through listening/speaking.
  useEffect(() => {
    const voice = previewMood === "listening" || previewMood === "speaking";
    if (!voice) {
      demoLevel.set(0);
      return;
    }
    if (reduced) {
      demoLevel.set(0.45);
      return;
    }
    let raf = 0;
    const t0 = performance.now();
    const tick = (now: number) => {
      const dt = (now - t0) / 1000;
      const v = previewMood === "speaking"
        ? 0.34 + 0.28 * Math.sin(dt * 9.5) + 0.16 * Math.sin(dt * 4.1)
        : 0.24 + 0.22 * Math.sin(dt * 6.2) + 0.12 * Math.cos(dt * 3.1);
      demoLevel.set(Math.max(0.08, Math.min(0.92, v)));
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [previewMood, reduced, demoLevel]);

  return (
    <div className="skin-gallery">
      <div className="skin-bar">
        <p className="skin-bar-note">
          {activeDemo ? (
            <>
              Сейчас на превью: <strong className="skin-mood-pill">{activeDemo.label}</strong>
            </>
          ) : (
            "Как выглядят глаза на острове и в окнах Джарвиса."
          )}
        </p>
        <button
          type="button"
          className={`btn skin-demo-btn${cycling ? " is-active" : ""}`}
          aria-pressed={cycling}
          onClick={() => setCycling((v) => !v)}
        >
          {cycling ? "Остановить" : "Показать эмоции"}
        </button>
      </div>

      <div className="skin-grid" role="radiogroup" aria-label="Облик Джарвиса">
        {SKINS.map((s) => {
          const selected = value === s.id;
          return (
            <button
              key={s.id}
              type="button"
              role="radio"
              aria-checked={selected}
              className={`skin-card${selected ? " is-on" : ""}`}
              style={{ ["--card-accent" as string]: s.accent, ["--card-rgb" as string]: s.rgb }}
              onClick={() => onChange(s.id)}
            >
              <span className="skin-stage" aria-hidden="true">
                <JarvisFace skin={s.id} mood={previewMood} level={demoLevel} scale={1.14} />
              </span>
              <span className="skin-meta">
                <span className="skin-head">
                  <span className="skin-dot" aria-hidden="true" />
                  <span className="skin-name">{s.label}</span>
                </span>
                <span className="skin-desc">{s.desc}</span>
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
}
