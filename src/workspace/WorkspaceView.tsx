import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Icon } from "../ui/Icon";
import { useAgent } from "../store/agent";
import { useIsland } from "../store/island";
import { Markdown } from "./Markdown";
import { ToolRow } from "./ToolRow";
import { JobsStrip } from "../island/views/JobsStrip";
import { toggleVoice } from "../store/voice";
import "./workspace.css";

const PHASE_LABEL = { thinking: "Думает", acting: "Выполняет", verifying: "Проверяет" } as const;

function Composer() {
  const [text, setText] = useState("");
  const ref = useRef<HTMLTextAreaElement>(null);
  const active = useAgent((s) => s.activeTurn);
  const send = useAgent((s) => s.send);
  const cancel = useAgent((s) => s.cancel);
  const attachments = useIsland((s) => s.pendingAttachments);
  const setIsland = useIsland((s) => s.set);

  useEffect(() => {
    // The window becomes focusable a moment after the workspace opens.
    const id = window.setTimeout(() => ref.current?.focus(), 80);
    return () => window.clearTimeout(id);
  }, []);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "0px";
    el.style.height = `${Math.min(el.scrollHeight, 112)}px`;
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
          placeholder="Спросите или поручите что-нибудь"
          onChange={(e) => setText(e.target.value)}
          onKeyDown={onKey}
          spellCheck
        />
        <button className="mic-btn" onClick={() => void toggleVoice()} aria-label="Голосовой режим">
          <Icon name="mic" size={16} />
        </button>
        {active ? (
          <button className="send-btn is-stop" onClick={() => void cancel()} aria-label="Остановить">
            <Icon name="stop" size={14} />
          </button>
        ) : (
          <button className="send-btn" onClick={submit} disabled={!text.trim()} aria-label="Отправить">
            <Icon name="send" size={16} />
          </button>
        )}
      </div>
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
        <button className="btn btn-solid" onClick={() => void invoke("settings_open")}>
          Открыть настройки
        </button>
      </div>
    );
  }
  return (
    <div className="empty">
      <p className="empty-title">Чем помочь?</p>
      <p className="empty-text">
        Опишите результат — например, «сделай громкость 30% и открой проводник» или «что сейчас на экране?».
      </p>
    </div>
  );
}

export function WorkspaceView() {
  const items = useAgent((s) => s.items);
  const phase = useAgent((s) => s.phase);
  const active = useAgent((s) => s.activeTurn);
  const status = useAgent((s) => s.status);
  const newConversation = useAgent((s) => s.newConversation);
  const set = useIsland((s) => s.set);
  const body = useRef<HTMLDivElement>(null);
  const stick = useRef(true);

  useLayoutEffect(() => {
    const el = body.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  }, [items]);

  return (
    <div className="view view-workspace">
      <div className="brow">
        <span className="brow-name">
          {active && phase ? `${PHASE_LABEL[phase]}…` : status?.configured ? status.model : "Джарвис"}
        </span>
        <div className="brow-actions">
          <button className="icon-btn" aria-label="Новый разговор" onClick={() => void newConversation()} disabled={!!active || items.length === 0}>
            <Icon name="chat" size={16} />
          </button>
          <button className="icon-btn" aria-label="Настройки" onClick={() => void invoke("settings_open")}>
            <Icon name="settings" size={16} />
          </button>
          <button className="icon-btn" aria-label="Свернуть" onClick={() => set({ workspaceOpen: false, hovered: false })}>
            <Icon name="collapse" size={16} />
          </button>
        </div>
      </div>

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
                  <div key={it.id} className={`msg-assistant${it.streaming ? " is-streaming" : ""}`}>
                    <Markdown text={it.text} />
                  </div>
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
      <Composer />
    </div>
  );
}
