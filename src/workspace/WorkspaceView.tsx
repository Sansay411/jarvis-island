import { memo, useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Icon } from "../ui/Icon";
import { useAgent, type ChatItem } from "../store/agent";
import { useIsland } from "../store/island";
import { usePrefs } from "../store/prefs";
import {
  ensureWorkspaceFeeds,
  hasTauri,
  stripMarkdownForSpeech,
  useWorkspaceTts,
  workspaceIpc,
} from "../lib/workspace-ipc";
import { Markdown } from "./Markdown";
import { ToolRow } from "./ToolRow";
import { MissionPlanCard } from "./MissionPlanCard";
import { RoutinesBar } from "./RoutinesBar";
import { LiveAudioBars, WsIcon } from "./WorkspaceIcons";
import { JobsStrip } from "../island/views/JobsStrip";
import { toggleVoice } from "../store/voice";
import "./workspace.css";

const PHASE_LABEL = { thinking: "Думает", acting: "Выполняет", verifying: "Проверяет" } as const;

/** Seconds left until `until` (epoch ms), ticking once a second. */
function useSecondsLeft(until: number | null): number {
  const [, tick] = useState(0);
  useEffect(() => {
    if (!until) return;
    const id = window.setInterval(() => tick((n) => n + 1), 1000);
    return () => window.clearInterval(id);
  }, [until]);
  return until ? Math.max(0, Math.ceil((until - Date.now()) / 1000)) : 0;
}

function Composer({ visible }: { visible: boolean }) {
  const [text, setText] = useState("");
  const ref = useRef<HTMLTextAreaElement>(null);
  const active = useAgent((s) => s.activeTurn);
  const send = useAgent((s) => s.send);
  const cancel = useAgent((s) => s.cancel);
  const attachments = useIsland((s) => s.pendingAttachments);
  const setIsland = useIsland((s) => s.set);

  useEffect(() => {
    if (!visible) return;
    const focus = () => ref.current?.focus();
    window.addEventListener("jarvis:input-ready", focus);
    const id = window.requestAnimationFrame(focus);
    return () => {
      window.cancelAnimationFrame(id);
      window.removeEventListener("jarvis:input-ready", focus);
    };
  }, [visible]);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "0px";
    el.style.height = `${Math.min(el.scrollHeight, 104)}px`;
  }, [text]);

  const submit = () => {
    const t = text.trim();
    if (!t || active) return;
    void send(t, attachments.map((a) => a.path));
    setText("");
    setIsland({ pendingAttachments: [] });
  };

  const onKey = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      submit();
    }
  };

  return (
    <div className="composer">
      {attachments.length > 0 && (
        <div className="attachments">
          {attachments.map((a) => (
            <span key={a.path} className="chip" title={a.path}>
              <Icon name="file" size={13} />
              {a.name}
              <button
                type="button"
                aria-label={`Убрать ${a.name}`}
                onClick={() => setIsland({ pendingAttachments: attachments.filter((x) => x.path !== a.path) })}
              >
                <Icon name="close" size={11} />
              </button>
            </span>
          ))}
        </div>
      )}
      <div className="composer-box">
        <textarea
          ref={ref}
          rows={1}
          value={text}
          placeholder="Спросите или поручите задачу Джарвису…"
          onChange={(e) => setText(e.target.value)}
          onKeyDown={onKey}
          spellCheck
        />
        <button type="button" className="mic-btn" onClick={() => void toggleVoice()} aria-label="Голосовой режим" title="Голосовой режим">
          <Icon name="mic" size={16} />
        </button>
        {active ? (
          <button type="button" className="send-btn is-stop" onClick={() => void cancel()} aria-label="Остановить" title="Остановить выполнение">
            <Icon name="stop" size={14} />
          </button>
        ) : (
          <button type="button" className="send-btn" onClick={submit} disabled={!text.trim()} aria-label="Отправить" title="Отправить (Enter)">
            <Icon name="send" size={16} />
          </button>
        )}
      </div>
    </div>
  );
}

function AssistantMessage({
  item,
  speakingId,
  onSpeakStart,
}: {
  item: Extract<ChatItem, { kind: "assistant" }>;
  speakingId: string | null;
  onSpeakStart: (id: string, text: string) => void;
}) {
  const ttsSpeaking = useWorkspaceTts((s) => s.speaking);
  const ttsText = useWorkspaceTts((s) => s.text);
  const ttsLatency = useWorkspaceTts((s) => s.latencyMs);
  const [copied, setCopied] = useState(false);

  const cleaned = stripMarkdownForSpeech(item.text);
  const prefixMatch =
    ttsSpeaking &&
    cleaned.length > 0 &&
    ttsText.length > 0 &&
    cleaned.slice(0, 28).toLowerCase() === ttsText.slice(0, 28).toLowerCase();
  const isSpeaking = ttsSpeaking && (speakingId === item.id || prefixMatch);

  const handleToggleSpeak = () => {
    if (isSpeaking) {
      void workspaceIpc.tts.stop();
    } else {
      onSpeakStart(item.id, item.text);
    }
  };

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(item.text);
    } catch {
      /* ignore in restricted frame */
    }
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1800);
  };

  return (
    <div className={`msg-assistant${item.streaming ? " is-streaming" : ""}${isSpeaking ? " is-speaking" : ""}`}>
      <Markdown text={item.text} />
      {!item.streaming && item.text.trim().length > 0 && (
        <div className="msg-actions">
          <button
            type="button"
            className={`msg-action-btn${isSpeaking ? " is-active" : ""}`}
            onClick={handleToggleSpeak}
            aria-label={isSpeaking ? "Остановить озвучку" : "Озвучить сообщение"}
          >
            {isSpeaking ? (
              <>
                <LiveAudioBars active />
                <span>Стоп</span>
              </>
            ) : (
              <>
                <WsIcon name="speaker" size={12} />
                <span>Озвучить</span>
              </>
            )}
          </button>

          {isSpeaking && ttsLatency !== null && (
            <span className="msg-tts-latency" title="Время до первого звука (Edge Neural HD)">
              ⚡ {ttsLatency} мс
            </span>
          )}

          <button
            type="button"
            className={`msg-action-btn${copied ? " is-copied" : ""}`}
            onClick={() => void handleCopy()}
            aria-label="Скопировать ответ"
          >
            <WsIcon name={copied ? "check" : "copy"} size={12} />
            <span>{copied ? "Скопировано" : "Копировать"}</span>
          </button>
        </div>
      )}
    </div>
  );
}

function EmptyState() {
  const status = useAgent((s) => s.status);
  if (status && !status.configured) {
    return (
      <div className="empty">
        <p className="empty-title">Подключите модель, чтобы начать</p>
        <p className="empty-text">{status.problem ?? "Укажите провайдера ИИ и API-ключ в настройках."}</p>
        <button
          type="button"
          className="btn btn-solid"
          onClick={() => {
            if (hasTauri()) void invoke("settings_open");
          }}
        >
          Открыть настройки
        </button>
      </div>
    );
  }
  return <RoutinesBar variant="empty" />;
}

export const WorkspaceView = memo(function WorkspaceView({ visible = true }: { visible?: boolean }) {
  const items = useAgent((s) => s.items);
  const phase = useAgent((s) => s.phase);
  const active = useAgent((s) => s.activeTurn);
  const status = useAgent((s) => s.status);
  const planSteps = useAgent((s) => s.planSteps);
  const waitLeft = useSecondsLeft(useAgent((s) => s.waitUntil));
  const newConversation = useAgent((s) => s.newConversation);
  const set = useIsland((s) => s.set);

  const fullControl = usePrefs((s) => s.fullControl);
  const setFullControl = usePrefs((s) => s.setFullControl);
  const autoSpeakReplies = usePrefs((s) => s.autoSpeakReplies);
  const setAutoSpeakReplies = usePrefs((s) => s.setAutoSpeakReplies);

  const ttsSpeaking = useWorkspaceTts((s) => s.speaking);
  const [speakingMsgId, setSpeakingMsgId] = useState<string | null>(null);

  const body = useRef<HTMLDivElement>(null);
  const stick = useRef(true);

  useEffect(() => {
    void ensureWorkspaceFeeds();
  }, []);

  useEffect(() => {
    if (!ttsSpeaking) setSpeakingMsgId(null);
  }, [ttsSpeaking]);

  useLayoutEffect(() => {
    const el = body.current;
    if (visible && el && stick.current) el.scrollTop = el.scrollHeight;
  }, [items, planSteps, visible]);

  const handleSpeakMessage = (id: string, text: string) => {
    setSpeakingMsgId(id);
    void workspaceIpc.tts.speak(text, { interrupt: true });
  };

  const statusText =
    active && waitLeft > 0
      ? `Лимит · ${waitLeft} с`
      : active && phase
        ? `${PHASE_LABEL[phase]}…`
        : status?.configured && status.model
          ? status.model
          : "Джарвис";

  return (
    <div className="view view-workspace">
      <div className="brow ws-brow">
        <div className="ws-brow-left">
          <span className={`ws-status-pill${active ? " is-busy" : ""}`} title={statusText}>
            <span className="ws-status-dot" />
            <span className="brow-name">{statusText}</span>
          </span>

          <button
            type="button"
            role="switch"
            aria-checked={fullControl}
            className={`ws-autopilot-pill${fullControl ? " is-on" : ""}`}
            onClick={() => void setFullControl(!fullControl)}
            title="Полное управление: агент действует сам; необратимые действия всё равно требуют подтверждения"
          >
            <span className="ws-autopilot-dot" />
            <span>Автопилот</span>
            <WsIcon name="bolt" size={11} className="ws-autopilot-bolt" />
          </button>
        </div>

        {/* Center 72px clear zone reserved for Jarvis's Eyes on the Island brow */}
        <div className="ws-brow-center" aria-hidden="true" />

        <div className="ws-brow-right">
          {ttsSpeaking && (
            <button
              type="button"
              className="ws-stop-tts-pill"
              onClick={() => void workspaceIpc.tts.stop()}
              title="Остановить озвучку (tts_stop)"
              aria-label="Стоп звук"
            >
              <LiveAudioBars active />
              <span>Стоп звук</span>
            </button>
          )}

          <button
            type="button"
            role="switch"
            aria-checked={autoSpeakReplies}
            className={`ws-autospeak-pill${autoSpeakReplies ? " is-on" : ""}${ttsSpeaking ? " is-compact" : ""}`}
            onClick={() => void setAutoSpeakReplies(!autoSpeakReplies)}
            title="Автоозвучка: автоматически озвучивать ответы Джарвиса в чате"
            aria-label="Автоозвучка"
          >
            <WsIcon name={autoSpeakReplies ? "speaker" : "speakerOff"} size={13} />
            {!ttsSpeaking && <span>Автоозвучка</span>}
          </button>

          <button
            type="button"
            className="icon-btn"
            aria-label="Новый разговор"
            title="Новый разговор"
            onClick={() => void newConversation()}
            disabled={!!active || (items.length === 0 && planSteps.length === 0)}
          >
            <Icon name="chat" size={15} />
          </button>
          <button
            type="button"
            className="icon-btn"
            aria-label="Настройки"
            title="Настройки Джарвиса"
            onClick={() => {
              if (hasTauri()) void invoke("settings_open");
            }}
          >
            <Icon name="settings" size={15} />
          </button>
          <button
            type="button"
            className="icon-btn"
            aria-label="Свернуть"
            title="Свернуть (Esc)"
            onClick={() => set({ workspaceOpen: false, hovered: false })}
          >
            <Icon name="collapse" size={15} />
          </button>
        </div>
      </div>

      {planSteps.length > 0 && (
        <div className="ws-plan-slot">
          <MissionPlanCard steps={planSteps} />
        </div>
      )}

      <div
        ref={body}
        className="ws-body scroll"
        onScroll={(e) => {
          const el = e.currentTarget;
          stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
        }}
      >
        {items.length === 0 ? (
          <EmptyState />
        ) : (
          items.map((it) => {
            switch (it.kind) {
              case "user":
                return (
                  <div key={it.id} className="msg-user">
                    <div className="bubble">{it.text}</div>
                    {it.attachments.length > 0 && (
                      <div className="msg-attachments">
                        {it.attachments.map((p) => (
                          <span key={p} className="chip" title={p}>
                            <Icon name="file" size={12} />
                            {p.split(/[\\/]/).pop()}
                          </span>
                        ))}
                      </div>
                    )}
                  </div>
                );
              case "assistant":
                return (
                  <AssistantMessage
                    key={it.id}
                    item={it}
                    speakingId={speakingMsgId}
                    onSpeakStart={handleSpeakMessage}
                  />
                );
              case "tool":
                return <ToolRow key={it.id} item={it} />;
              case "notice":
                return (
                  <div key={it.id} className={`notice notice-${it.tone}`}>
                    {it.tone === "error" && <Icon name="alert" size={14} />}
                    {it.text}
                  </div>
                );
            }
          })
        )}
      </div>

      <JobsStrip />
      {items.length > 0 && <RoutinesBar variant="strip" />}
      <Composer visible={visible} />
    </div>
  );
});
