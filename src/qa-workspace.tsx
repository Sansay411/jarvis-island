// Visual & Interactive QA Harness for WorkspaceView & SettingsApp.
// Served at `/qa/workspace-showcase.html` for live browser verification.

import { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { useMotionValue } from "motion/react";
import "./styles/tokens.css";
import "./styles/base.css";
import "./island/island.css";
import { WorkspaceView } from "./workspace/WorkspaceView";
import SettingsApp from "./settings/SettingsApp";
import type { ModelSectionQaMock } from "./settings/ModelSection";
import { Eyes } from "./island/Eyes";
import { BROW_H, bottomRadius, notchPath } from "./island/geometry";
import { useAgent, type ChatItem, type PlanStep } from "./store/agent";
import { usePrefs } from "./store/prefs";
import { ensureWorkspaceFeeds, useWorkspaceTts } from "./lib/workspace-ipc";

type QaScene = "workspace-active" | "workspace-empty" | "settings-voice" | "settings-behaviour" | "settings-skills" | "settings-model" | "settings-persona" | "settings-memory";

const QA_MODEL_MOCK: ModelSectionQaMock = {
  models: {
    serverKind: "ollama",
    models: [
      {
        id: "qwen2.5:7b",
        vision: "no",
        tools: "yes",
        maxContext: 32768,
        loadedContext: 8192,
        state: "loaded",
        suitable: false,
        reason: "Контекст 8k < ~14k",
      },
      {
        id: "qwen2.5vl:7b",
        vision: "yes",
        tools: "yes",
        maxContext: 32768,
        loadedContext: 32768,
        state: "loaded",
        suitable: true,
        reason: null,
      },
      {
        id: "nomic-embed-text:latest",
        vision: "no",
        tools: "no",
        maxContext: 2048,
        loadedContext: null,
        state: null,
        suitable: false,
        reason: "Не вызывает инструменты",
      },
    ],
  },
  probe: {
    vision: "no",
    tools: "yes",
    context: { modelMax: 32768, loaded: 8192, needed: 14120, source: "ps" },
    verdict: "bad",
    summary: "Контекста 8k не хватает для инструкций и инструментов (нужно ~14k).",
    notes: [
      "Для вопросов про экран выберите модель с картинками (например Qwen-VL) в поле «Модель для экрана».",
      "В настройках Ollama поставьте Context length 32k или запустите с OLLAMA_CONTEXT_LENGTH=32768.",
    ],
    serverKind: "ollama",
  },
};

const SAMPLE_PLAN: PlanStep[] = [
  { id: "s1", title: "Снимок раскладки окон и диагностика ресурсов ПК", status: "done" },
  { id: "s2", title: "Тайлинг VS Code (65%) и браузера документации (35%)", status: "running" },
  { id: "s3", title: "Запуск сессии глубокого фокуса на 45 минут", status: "pending" },
  { id: "s4", title: "Голосовая сводка о готовности рабочего места", status: "pending" },
];

const SAMPLE_ITEMS: ChatItem[] = [
  {
    kind: "user",
    id: "u1",
    text: "Включи режим глубокой работы: разложи VS Code и браузер, проверь нагрузку ПК и доложи голосом.",
    attachments: [],
  },
  {
    kind: "tool",
    id: "t1",
    turnId: "turn-qa-1",
    tool: "system_diagnostics",
    title: "Диагностика ресурсов системы",
    status: "ok",
    args: { topProcesses: 5 },
    summary: "CPU 9% · ОЗУ 13,8 / 32 ГБ · C: свободно 418 ГБ",
  },
  {
    kind: "tool",
    id: "t2",
    turnId: "turn-qa-1",
    tool: "window_layout",
    title: "Тайлинг активных окон 65 / 35",
    status: "ok",
    args: { action: "tile_pair", ratio: "65:35", left: "Code.exe", right: "msedge.exe" },
    summary: "VS Code (слева 65%) · Браузер (справа 35%)",
  },
  {
    kind: "assistant",
    id: "a1",
    turnId: "turn-qa-1",
    text: "Рабочее место подготовлено:\n- **Окна**: `VS Code` и браузер выровнены в пропорции **65/35**, второстепенные окна свёрнуты.\n- **Система**: нагрузка процессора **9%**, свободно **18,2 ГБ** оперативной памяти.\n- **Фокус**: таймер на **45 минут** запущен.",
    streaming: false,
  },
];

const CONFIRM_ITEM: ChatItem = {
  kind: "tool",
  id: "t-confirm",
  turnId: "turn-qa-1",
  tool: "shell",
  title: "Очистка кэша временных сборок (target/debug)",
  status: "awaiting",
  args: null,
  risk: "moderate",
  details: "Remove-Item -Recurse -Force .\\target\\debug\\incremental",
};

function seedActiveWorkspace(withConfirm = false) {
  usePrefs.setState({
    fullControl: true,
    autoSpeakReplies: true,
  });
  useAgent.setState({
    status: { configured: true, provider: "DashScope", model: "qwen3.8-max", problem: null },
    activeTurn: null,
    phase: null,
    planTurnId: "turn-qa-1",
    planSteps: SAMPLE_PLAN,
    items: withConfirm ? [...SAMPLE_ITEMS, CONFIRM_ITEM] : SAMPLE_ITEMS,
  });
}

function seedEmptyWorkspace() {
  usePrefs.setState({
    fullControl: true,
    autoSpeakReplies: true,
  });
  useAgent.setState({
    status: { configured: true, provider: "DashScope", model: "qwen3.8-max", problem: null },
    activeTurn: null,
    phase: null,
    planTurnId: null,
    planSteps: [],
    items: [],
  });
}

function IslandWorkspaceFrame() {
  const ttsSpeaking = useWorkspaceTts((s) => s.speaking);
  const ttsLevel = useWorkspaceTts((s) => s.level);
  const level = useMotionValue(0);
  const zero = useMotionValue(0);
  const [vw, setVw] = useState(() => (typeof window !== "undefined" ? window.innerWidth : 1280));

  useEffect(() => {
    const onResize = () => setVw(window.innerWidth);
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  useEffect(() => {
    level.set(ttsSpeaking ? Math.max(0.28, ttsLevel) : 0);
  }, [ttsSpeaking, ttsLevel, level]);

  const w = Math.min(548, Math.max(432, vw - 28));
  const h = 516;
  const s = 14;
  const r = bottomRadius(h);
  const d = notchPath(w, h, r, s);

  return (
    <div
      style={{
        position: "relative",
        width: "100%",
        height: "100%",
        background: "radial-gradient(90% 60% at 50% 0%, #1c2029 0%, #0d0f14 70%, #090a0d 100%)",
        overflow: "hidden",
      }}
    >
      <div
        className="island"
        style={{
          width: w,
          height: h,
          top: 0,
          left: "50%",
          translate: "-50% 0",
          position: "absolute",
        }}
      >
        <svg
          className="island-shape"
          style={{ width: w + s * 2, height: h, left: -s }}
          aria-hidden="true"
        >
          <defs>
            <linearGradient id="qa-rim" x1="0" y1="0" x2="0" y2="1">
              <stop offset="0" stopColor="#ffffff" stopOpacity="0" />
              <stop offset="0.6" stopColor="#ffffff" stopOpacity="0.05" />
              <stop offset="1" stopColor="#ffffff" stopOpacity="0.16" />
            </linearGradient>
            <radialGradient id="qa-sheen" cx="0.5" cy="0" r="0.9">
              <stop offset="0" stopColor="#1b1b20" />
              <stop offset="0.7" stopColor="#0a0a0d" />
              <stop offset="1" stopColor="#070709" />
            </radialGradient>
          </defs>
          <path d={d} fill="url(#qa-sheen)" stroke="url(#qa-rim)" strokeWidth={1} />
        </svg>

        <div
          className="island-clip"
          style={{ borderBottomLeftRadius: r, borderBottomRightRadius: r }}
        >
          <div className="island-content" style={{ width: w, height: h }}>
            <WorkspaceView visible />
          </div>
        </div>

        <div className="island-eyes" style={{ height: BROW_H }}>
          <Eyes
            mood={ttsSpeaking ? "speaking" : "attentive"}
            level={level}
            lookX={zero}
            lookY={0}
            scale={1}
          />
        </div>
      </div>
    </div>
  );
}

function getInitialScene(): QaScene {
  if (typeof window === "undefined") return "workspace-active";
  const p = new URLSearchParams(window.location.search).get("scene");
  if (
    p === "workspace-empty" ||
    p === "settings-voice" ||
    p === "settings-behaviour" ||
    p === "settings-skills" ||
    p === "settings-model" ||
    p === "settings-persona" ||
    p === "settings-memory" ||
    p === "workspace-active"
  ) {
    return p;
  }
  return "workspace-active";
}

function QaWorkspaceApp() {
  const [scene, setScene] = useState<QaScene>(() => getInitialScene());
  const [withConfirm, setWithConfirm] = useState(false);

  useEffect(() => {
    void ensureWorkspaceFeeds();
    const initial = getInitialScene();
    if (initial === "workspace-empty") seedEmptyWorkspace();
    else seedActiveWorkspace(false);
  }, []);

  const selectScene = (next: QaScene) => {
    setScene(next);
    if (next === "workspace-active") seedActiveWorkspace(withConfirm);
    if (next === "workspace-empty") seedEmptyWorkspace();
  };

  const toggleConfirm = () => {
    const next = !withConfirm;
    setWithConfirm(next);
    setScene("workspace-active");
    seedActiveWorkspace(next);
  };

  return (
    <div style={{ width: "100vw", height: "100vh", position: "relative", overflow: "hidden" }}>
      {scene === "workspace-active" || scene === "workspace-empty" ? (
        <IslandWorkspaceFrame />
      ) : scene === "settings-voice" ? (
        <SettingsApp initialSection="voice" />
      ) : scene === "settings-skills" ? (
        <SettingsApp initialSection="skills" />
      ) : scene === "settings-model" ? (
        <SettingsApp
          initialSection="model"
          qaInitialProvider={{ preset: "ollama", baseUrl: "", model: "qwen2.5:7b", visionModel: "" }}
          qaModelMock={QA_MODEL_MOCK}
        />
      ) : scene === "settings-persona" ? (
        <SettingsApp initialSection="persona" />
      ) : scene === "settings-memory" ? (
        <SettingsApp initialSection="memory" />
      ) : (
        <SettingsApp initialSection="behaviour" />
      )}

      {/* Compact QA Scene Switcher Dock */}
      <nav
        aria-label="Переключатель сцен QA"
        style={{
          position: "fixed",
          bottom: 14,
          left: "50%",
          transform: "translateX(-50%)",
          zIndex: 9999,
          display: "flex",
          flexWrap: "wrap",
          alignItems: "center",
          gap: 6,
          padding: "6px 8px",
          borderRadius: 999,
          background: "rgba(12, 12, 15, 0.92)",
          border: "1px solid rgba(255, 255, 255, 0.14)",
          boxShadow: "0 14px 36px rgba(0, 0, 0, 0.55)",
          backdropFilter: "blur(12px)",
        }}
      >
        {(
          [
            ["workspace-active", "1 · Чат + План + TTS"],
            ["workspace-empty", "2 · Пустой чат + Сценарии"],
            ["settings-voice", "3 · Настройки: Голос HD"],
            ["settings-behaviour", "4 · Настройки: Автопилот"],
            ["settings-skills", "5 · Настройки: Навыки"],
            ["settings-model", "6 · Настройки: Модель"],
            ["settings-persona", "7 · Настройки: Облик и Характер"],
            ["settings-memory", "8 · Настройки: Память и уроки"],
          ] as [QaScene, string][]
        ).map(([id, label]) => {
          const on = scene === id;
          return (
            <button
              key={id}
              type="button"
              onClick={() => selectScene(id)}
              style={{
                height: 26,
                padding: "0 11px",
                borderRadius: 999,
                fontSize: 11.5,
                fontWeight: 560,
                background: on ? "#f2f2f0" : "transparent",
                color: on ? "#08080a" : "#a4a4aa",
              }}
            >
              {label}
            </button>
          );
        })}

        {(scene === "workspace-active" || scene === "workspace-empty") && (
          <button
            type="button"
            onClick={toggleConfirm}
            style={{
              height: 26,
              padding: "0 10px",
              borderRadius: 999,
              fontSize: 11,
              fontWeight: 560,
              background: withConfirm ? "rgba(255, 178, 63, 0.18)" : "rgba(255, 255, 255, 0.06)",
              color: withConfirm ? "#ffd88f" : "#a4a4aa",
              border: withConfirm ? "1px solid rgba(255, 178, 63, 0.45)" : "1px solid transparent",
            }}
          >
            {withConfirm ? "✓ Карточка разрешения" : "+ Карточка разрешения"}
          </button>
        )}
      </nav>
    </div>
  );
}

createRoot(document.getElementById("root")!).render(<QaWorkspaceApp />);
