// Visual & interactive QA for «Знакомство» (`qa/welcome.html?step=0..5`,
// plus `step=6` or `step=done` for the final tips screen). Renders the real
// WelcomeApp inside a 640×560 window frame with mocked IPC and a bottom step
// switcher dock.

import { StrictMode, useState } from "react";
import { createRoot } from "react-dom/client";
import "@fontsource-variable/onest";
import "@fontsource-variable/jetbrains-mono";
import "./styles/tokens.css";
import "./styles/base.css";
import WelcomeApp from "./welcome/WelcomeApp";
import type { OnboardingStep } from "./welcome/onboarding";
import type { Settings } from "./settings/SettingsApp";

const LABELS: Array<[OnboardingStep, string]> = [
  [0, "0 · Привет"],
  [1, "1 · Имя"],
  [2, "2 · Занятие"],
  [3, "3 · Цели"],
  [4, "4 · Ритм"],
  [5, "5 · Голос"],
  [6, "Готово"],
];

function readInitialStep(): OnboardingStep {
  if (typeof window === "undefined") return 0;
  const raw = new URLSearchParams(window.location.search).get("step");
  if (raw === "done") return 6;
  const n = Number(raw);
  if (Number.isInteger(n) && n >= 0 && n <= 6) return n as OnboardingStep;
  return 0;
}

function QaWelcome() {
  const [step, setStep] = useState<OnboardingStep>(readInitialStep);
  const [saved, setSaved] = useState<{ settings: Settings; facts: string[] } | null>(null);

  const pick = (s: OnboardingStep) => {
    setStep(s);
    const url = new URL(window.location.href);
    url.searchParams.set("step", String(s));
    window.history.replaceState(null, "", url);
  };

  return (
    <div
      style={{
        width: "100vw",
        height: "100vh",
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        gap: 14,
        background: "radial-gradient(80% 55% at 50% 10%, #17181f 0%, #090a0d 72%)",
        padding: 16,
      }}
    >
      <div
        style={{
          width: "min(640px, 100%)",
          height: "min(560px, calc(100vh - 84px))",
          borderRadius: 14,
          overflow: "hidden",
          border: "1px solid rgba(255, 255, 255, 0.12)",
          boxShadow: "0 28px 80px rgba(0, 0, 0, 0.72)",
          position: "relative",
        }}
      >
        <WelcomeApp initialStep={step} qaMode onQaSaved={setSaved} />
      </div>

      {saved && (
        <div
          style={{
            maxWidth: 640,
            fontSize: 11.5,
            color: "#a4a4aa",
            background: "rgba(16, 16, 19, 0.9)",
            border: "1px solid rgba(255, 255, 255, 0.1)",
            borderRadius: 10,
            padding: "8px 12px",
          }}
        >
          Сохранено (мок): имя «{saved.settings.persona.userName || "—"}», треки [{saved.settings.mentor.tracks.join(", ")}], память:{" "}
          {saved.facts.length > 0 ? saved.facts.join(" · ") : "пусто"}
        </div>
      )}

      <nav
        aria-label="Переключатель шагов QA"
        style={{
          display: "flex",
          flexWrap: "wrap",
          alignItems: "center",
          gap: 5,
          padding: "5px 7px",
          borderRadius: 999,
          background: "rgba(12, 12, 15, 0.92)",
          border: "1px solid rgba(255, 255, 255, 0.14)",
          boxShadow: "0 12px 32px rgba(0, 0, 0, 0.5)",
        }}
      >
        {LABELS.map(([id, label]) => {
          const on = step === id;
          return (
            <button
              key={id}
              type="button"
              onClick={() => pick(id)}
              style={{
                height: 26,
                padding: "0 11px",
                borderRadius: 999,
                fontSize: 11.5,
                fontWeight: 560,
                background: on ? "#ecebe6" : "transparent",
                color: on ? "#08080a" : "#a4a4aa",
              }}
            >
              {label}
            </button>
          );
        })}
      </nav>
    </div>
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <QaWelcome />
  </StrictMode>,
);
