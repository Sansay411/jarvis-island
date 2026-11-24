// Secure key prompt shown in the island when `connect_service` needs an API
// token or secret header. Values go straight to `mcp_secret_submit` and Windows
// Credential Manager — never through the chat history or the model.

import { useEffect, useLayoutEffect, useRef, useState, type FormEvent } from "react";
import { Icon } from "../../ui/Icon";
import { WsIcon } from "../../workspace/WorkspaceIcons";
import { ipc } from "../../lib/ipc";
import { useMcpPrompt } from "../../store/mcpPrompt";

export function SecretPromptView() {
  const prompt = useMcpPrompt((s) => s.prompt);
  const submitting = useMcpPrompt((s) => s.submitting);
  const error = useMcpPrompt((s) => s.error);
  const submit = useMcpPrompt((s) => s.submit);
  const cancel = useMcpPrompt((s) => s.cancel);
  const setHeight = useMcpPrompt((s) => s.setHeight);

  const [values, setValues] = useState<Record<string, string>>({});
  const [revealed, setRevealed] = useState<Record<string, boolean>>({});
  const rootRef = useRef<HTMLFormElement>(null);
  const firstInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    setValues({});
    setRevealed({});
  }, [prompt?.promptId]);

  useEffect(() => {
    const focus = () => firstInputRef.current?.focus();
    window.addEventListener("jarvis:input-ready", focus);
    const id = window.requestAnimationFrame(focus);
    return () => {
      window.cancelAnimationFrame(id);
      window.removeEventListener("jarvis:input-ready", focus);
    };
  }, [prompt?.promptId]);

  useLayoutEffect(() => {
    const el = rootRef.current;
    if (!el) return;
    const measure = () => setHeight(Math.ceil(el.scrollHeight));
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, [prompt?.promptId, error, setHeight]);

  if (!prompt) return null;

  const ready = prompt.fields.every((f) => f.optional || (values[f.key] ?? "").trim().length > 0);

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    if (!ready || submitting) return;
    const trimmed: Record<string, string> = {};
    for (const f of prompt.fields) {
      const v = (values[f.key] ?? "").trim();
      if (v) trimmed[f.key] = v;
    }
    void submit(trimmed);
  };

  return (
    <form ref={rootRef} className="view view-secret" onSubmit={onSubmit} aria-label={`Ключ доступа для ${prompt.serviceName}`}>
      <div className="brow secret-brow">
        <div className="secret-brow-left">
          <WsIcon name="shield" size={13} className="secret-shield" />
          <span className="brow-name">Подключить {prompt.serviceName}</span>
        </div>
        <div className="secret-brow-center" aria-hidden="true" />
        <div className="secret-brow-right">
          <button
            type="button"
            className="icon-btn"
            aria-label="Отмена"
            title="Отмена (Esc)"
            onClick={() => void cancel()}
          >
            <Icon name="close" size={14} />
          </button>
        </div>
      </div>

      <div className="secret-body">
        {prompt.note && <p className="secret-note">{prompt.note}</p>}

        {prompt.fields.map((f, idx) => {
          const shown = Boolean(revealed[f.key]);
          return (
            <div key={f.key} className="secret-field">
              <div className="secret-field-head">
                <label className="secret-label" htmlFor={`mcp-secret-${f.key}`}>
                  {f.label}
                </label>
                {f.link && (
                  <a className="secret-link" href={f.link} target="_blank" rel="noreferrer">
                    Где взять ↗
                  </a>
                )}
              </div>
              <div className="secret-input-wrap">
                <input
                  ref={idx === 0 ? firstInputRef : undefined}
                  id={`mcp-secret-${f.key}`}
                  className="secret-input"
                  type={shown ? "text" : "password"}
                  autoComplete="off"
                  spellCheck={false}
                  value={values[f.key] ?? ""}
                  placeholder={f.placeholder ?? "Вставьте ключ или токен…"}
                  onFocus={() => void ipc.island.beginInput()}
                  onChange={(e) => setValues((v) => ({ ...v, [f.key]: e.target.value }))}
                />
                <button
                  type="button"
                  className="secret-reveal-btn"
                  aria-label={shown ? "Скрыть ключ" : "Показать ключ"}
                  title={shown ? "Скрыть" : "Показать"}
                  onClick={() => setRevealed((r) => ({ ...r, [f.key]: !r[f.key] }))}
                >
                  <Icon name="eye" size={13} />
                </button>
              </div>
              {f.hint && <p className="secret-hint">{f.hint}</p>}
            </div>
          );
        })}

        {error && <p className="secret-error">{error}</p>}

        <div className="secret-foot">
          <span className="secret-guard">Сохранится в диспетчере учётных данных и не попадёт в чат</span>
          <div className="secret-actions">
            <button type="button" className="btn btn-quiet" disabled={submitting} onClick={() => void cancel()}>
              Отмена
            </button>
            <button type="submit" className="btn btn-solid secret-save-btn" disabled={!ready || submitting}>
              {submitting ? "Сохраняю…" : "Сохранить"}
            </button>
          </div>
        </div>
      </div>
    </form>
  );
}
