// Settings -> «Модель»: provider picker, local/custom model dropdown with
// capability metadata, debounced probe card, and optional active check.

import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { errorMessage } from "../lib/ipc";
import { hasTauri } from "../lib/workspace-ipc";
import { Row } from "./controls";
import {
  buildProbeBadges,
  isLocalOrCustomPreset,
  type ModelOption,
  type ModelProbe,
  type ModelsListResult,
} from "./modelProbe";

export interface ProviderPreset {
  id: string;
  label: string;
  baseUrl: string;
  defaultModel: string;
  needsKey: boolean;
  dashscope: boolean;
}

export interface ProviderDraft {
  preset: string;
  baseUrl: string;
  model: string;
  visionModel: string;
  temperature: number;
}

export interface ModelSectionQaMock {
  models?: ModelsListResult | null;
  probe?: ModelProbe | null;
  probeError?: string | null;
}

function modelFilter(models: ModelOption[], query: string, showAll: boolean): ModelOption[] {
  const q = query.trim().toLowerCase();
  if (showAll || !q) return models;
  const filtered = models.filter((m) => m.id.toLowerCase().includes(q));
  return filtered.length > 0 ? filtered : models;
}

function ModelCombobox({
  value,
  placeholder,
  models,
  loading,
  onChange,
  onRefresh,
}: {
  value: string;
  placeholder: string;
  models: ModelOption[];
  loading: boolean;
  onChange: (next: string) => void;
  onRefresh: () => void;
}) {
  const [open, setOpen] = useState(false);
  const [showAll, setShowAll] = useState(true);
  const wrapRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (wrapRef.current && !wrapRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    window.addEventListener("mousedown", onDown);
    return () => window.removeEventListener("mousedown", onDown);
  }, [open]);

  const visible = useMemo(() => modelFilter(models, value, showAll), [models, value, showAll]);

  return (
    <div className="model-combo" ref={wrapRef}>
      <div className="inline">
        <input
          className="mono"
          value={value}
          placeholder={placeholder}
          spellCheck={false}
          aria-expanded={open}
          aria-autocomplete="list"
          onFocus={() => {
            if (models.length > 0) {
              setShowAll(true);
              setOpen(true);
            }
          }}
          onChange={(e) => {
            onChange(e.target.value);
            setShowAll(false);
            if (models.length > 0) setOpen(true);
          }}
        />
        {models.length > 0 && (
          <button
            type="button"
            className="btn model-list-btn"
            title="Выбрать из списка моделей сервера"
            aria-label="Список моделей"
            onClick={() => {
              setShowAll(true);
              setOpen((o) => !o);
            }}
          >
            <span>{models.length}</span>
            <span className={`model-caret${open ? " is-open" : ""}`} aria-hidden="true" />
          </button>
        )}
        <button
          type="button"
          className="btn"
          disabled={loading}
          title="Обновить список моделей с сервера"
          onClick={onRefresh}
        >
          {loading ? "…" : "Обновить"}
        </button>
      </div>

      {open && visible.length > 0 && (
        <ul className="model-menu" role="listbox" aria-label="Модели на сервере">
          {visible.map((m) => {
            const active = m.id === value;
            const note = m.reason ?? (m.vision === "yes" ? "видит экран" : undefined);
            return (
              <li key={m.id} role="option" aria-selected={active}>
                <button
                  type="button"
                  className={`model-opt${active ? " is-active" : ""}${!m.suitable ? " is-unsuitable" : ""}`}
                  title={m.reason ?? undefined}
                  onClick={() => {
                    onChange(m.id);
                    setOpen(false);
                  }}
                >
                  <span className="model-opt-id mono">{m.id}</span>
                  {note && (
                    <span className={`model-opt-tag${!m.suitable ? " is-bad" : m.vision === "yes" ? " is-vision" : ""}`}>
                      {note}
                    </span>
                  )}
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}

export function ModelSection({
  provider,
  presets,
  preset,
  hasLlmKey,
  busy,
  onPatchProvider,
  onTest,
  renderKeyField,
  qaMock,
}: {
  provider: ProviderDraft;
  presets: ProviderPreset[];
  preset: ProviderPreset;
  hasLlmKey: boolean;
  busy: boolean;
  onPatchProvider: (patch: Partial<ProviderDraft>) => void;
  onTest: () => void;
  renderKeyField: (baseUrl: string) => ReactNode;
  qaMock?: ModelSectionQaMock | null;
}) {
  const isLocal = isLocalOrCustomPreset(preset.id);
  const effectiveUrl = (provider.baseUrl.trim() || preset.baseUrl).replace(/\/+$/, "");

  const [modelsList, setModelsList] = useState<ModelsListResult | null>(qaMock?.models ?? null);
  const [modelsLoading, setModelsLoading] = useState(false);

  const [probe, setProbe] = useState<ModelProbe | null>(qaMock?.probe ?? null);
  const [probeLoading, setProbeLoading] = useState(false);
  const [activeLoading, setActiveLoading] = useState(false);
  const [probeError, setProbeError] = useState<string | null>(qaMock?.probeError ?? null);
  const probeSeq = useRef(0);

  useEffect(() => {
    if (!qaMock) return;
    if (qaMock.models !== undefined) setModelsList(qaMock.models);
    if (qaMock.probe !== undefined) setProbe(qaMock.probe);
    if (qaMock.probeError !== undefined) setProbeError(qaMock.probeError);
  }, [qaMock]);

  const fetchModels = useCallback(async () => {
    if (!isLocal || !effectiveUrl) {
      setModelsList(null);
      return;
    }
    if (!hasTauri()) return;
    setModelsLoading(true);
    try {
      const res = await invoke<ModelsListResult>("models_list", {
        preset: preset.id,
        baseUrl: effectiveUrl,
      });
      setModelsList(res);
    } catch {
      setModelsList(null);
    } finally {
      setModelsLoading(false);
    }
  }, [isLocal, effectiveUrl, preset.id]);

  const runProbe = useCallback(
    async (opts?: { active?: boolean; refresh?: boolean }) => {
      const model = provider.model.trim();
      if (!isLocal || !effectiveUrl || !model) {
        setProbe(null);
        setProbeError(null);
        return;
      }
      if (!hasTauri()) return;
      const seq = ++probeSeq.current;
      const isActive = !!opts?.active;
      if (isActive) setActiveLoading(true);
      else setProbeLoading(true);
      setProbeError(null);
      try {
        const res = await invoke<ModelProbe>("model_probe", {
          preset: preset.id,
          baseUrl: effectiveUrl,
          model,
          active: isActive,
          refresh: !!opts?.refresh,
        });
        if (probeSeq.current === seq) {
          setProbe(res);
        }
      } catch (e) {
        if (probeSeq.current === seq) {
          setProbe(null);
          setProbeError(errorMessage(e));
        }
      } finally {
        if (probeSeq.current === seq) {
          setProbeLoading(false);
          setActiveLoading(false);
        }
      }
    },
    [isLocal, effectiveUrl, preset.id, provider.model],
  );

  // Fetch installed models when switching to a local/custom preset or changing URL.
  useEffect(() => {
    if (!isLocal || !effectiveUrl) {
      setModelsList(null);
      return;
    }
    if (qaMock) return;
    const t = window.setTimeout(() => void fetchModels(), 250);
    return () => window.clearTimeout(t);
  }, [isLocal, effectiveUrl, fetchModels, qaMock]);

  // Automatically probe the selected model (debounced) for local/custom presets.
  useEffect(() => {
    if (!isLocal || !effectiveUrl || !provider.model.trim()) {
      setProbe(null);
      setProbeError(null);
      return;
    }
    if (qaMock) return;
    const t = window.setTimeout(() => void runProbe(), 450);
    return () => window.clearTimeout(t);
  }, [isLocal, effectiveUrl, provider.model, runProbe, qaMock]);

  const badges = useMemo(() => (probe ? buildProbeBadges(probe, provider.visionModel) : []), [probe, provider.visionModel]);
  const visionModels = useMemo(
    () => (modelsList?.models ?? []).filter((m) => m.vision === "yes" && m.id !== provider.model),
    [modelsList, provider.model],
  );
  const showVisionFieldOutside = isLocal && (probe?.vision === "no" || provider.visionModel.trim().length > 0);
  const showActiveButton =
    probe?.vision === "unknown" || probe?.tools === "unknown" || (!!probeError && preset.id === "custom");

  const apiUrlRow = (
    <Row
      label="Адрес API"
      hint={preset.baseUrl ? `Пусто — ${preset.baseUrl}` : "Укажите адрес OpenAI-совместимого API (например http://localhost:1234/v1)."}
    >
      <input
        className="mono"
        value={provider.baseUrl}
        placeholder={preset.baseUrl || "http://localhost:8080/v1"}
        spellCheck={false}
        onChange={(e) => onPatchProvider({ baseUrl: e.target.value })}
      />
    </Row>
  );

  const modelInputRow = (
    <Row
      label="Модель"
      hint={
        preset.id === "jarvis-cloud"
          ? "DeepSeek V4.1 Flash через Jarvis Cloud: без ключа, с дневным лимитом на устройство."
          : preset.id === "deepseek"
            ? "V4.1 Flash: быстрые ответы, понимание экрана и поиск в интернете."
            : preset.dashscope
              ? "qwen3.8-flash: быстрее, qwen3.8-max: сильнее."
              : preset.id === "ollama"
                ? "Установленные модели подгружаются из Ollama автоматически."
                : preset.id === "lmstudio"
                  ? "Список моделей берётся из локального сервера LM Studio."
                  : "Название модели на сервере (можно выбрать из списка или ввести вручную)."
      }
    >
      {isLocal ? (
        <ModelCombobox
          value={provider.model}
          placeholder="Выберите или введите модель"
          models={modelsList?.models ?? []}
          loading={modelsLoading}
          onChange={(m) => onPatchProvider({ model: m })}
          onRefresh={() => void fetchModels()}
        />
      ) : (
        <input
          className="mono"
          value={provider.model}
          spellCheck={false}
          onChange={(e) => onPatchProvider({ model: e.target.value })}
        />
      )}
    </Row>
  );

  const visionModelRow = (
    <Row label="Модель для экрана" hint="Для скриншотов и картинок. Пусто — используется основная модель.">
      <div className="vision-field">
        <input
          className="mono"
          value={provider.visionModel}
          placeholder="Например qwen2.5vl:7b"
          spellCheck={false}
          onChange={(e) => onPatchProvider({ visionModel: e.target.value })}
        />
        {visionModels.length > 0 && (
          <div className="vision-chips">
            {visionModels.slice(0, 4).map((m) => (
              <button
                key={m.id}
                type="button"
                className={`vision-chip${provider.visionModel === m.id ? " is-on" : ""}`}
                onClick={() => onPatchProvider({ visionModel: m.id })}
              >
                {m.id}
              </button>
            ))}
          </div>
        )}
      </div>
    </Row>
  );

  return (
    <div className="group">
      <div className="group-rows">
        <Row label="Провайдер">
          <select
            value={provider.preset}
            onChange={(e) => {
              const p = presets.find((x) => x.id === e.target.value)!;
              onPatchProvider({
                preset: p.id,
                baseUrl: "",
                model: p.defaultModel || (isLocalOrCustomPreset(p.id) ? "" : provider.model),
              });
            }}
          >
            {presets.map((p) => (
              <option key={p.id} value={p.id}>
                {p.label}
              </option>
            ))}
          </select>
        </Row>

        {preset.needsKey && (
          <>
            {renderKeyField(effectiveUrl)}
            {preset.id === "custom" && !hasLlmKey && (
              <p className="probe-key-hint">Для локальных адресов (localhost) ключ не обязателен.</p>
            )}
          </>
        )}

        {isLocal && preset.id === "custom" && apiUrlRow}
        {isLocal && modelInputRow}

        {isLocal && provider.model.trim() && (
          <div className={`probe-card${probe ? ` is-${probe.verdict}` : ""}`} role="region" aria-label="Проверка локальной модели">
            {probeLoading && !probe ? (
              <div className="probe-head">
                <span className="probe-status">
                  <span className="probe-dot is-loading" />
                  <span>Проверяю возможности модели…</span>
                </span>
              </div>
            ) : probeError ? (
              <div className="probe-head">
                <span className="probe-status is-error">{probeError}</span>
                <div className="probe-actions">
                  {showActiveButton && (
                    <button
                      type="button"
                      className="btn"
                      disabled={activeLoading}
                      onClick={() => void runProbe({ active: true, refresh: true })}
                    >
                      {activeLoading ? "Проверяю на деле…" : "Проверить на деле"}
                    </button>
                  )}
                  <button
                    type="button"
                    className="btn"
                    disabled={probeLoading}
                    onClick={() => void runProbe({ refresh: true })}
                  >
                    Повторить
                  </button>
                </div>
              </div>
            ) : probe ? (
              <>
                <div className="probe-head">
                  <span className="probe-status">
                    <span className={`probe-dot is-${probe.verdict}`} aria-hidden="true" />
                    <span>{probe.summary}</span>
                  </span>
                  <div className="probe-actions">
                    {showActiveButton && (
                      <button
                        type="button"
                        className="btn btn-primary"
                        disabled={activeLoading || probeLoading}
                        onClick={() => void runProbe({ active: true, refresh: true })}
                      >
                        {activeLoading ? "Проверяю на деле…" : "Проверить на деле"}
                      </button>
                    )}
                    <button
                      type="button"
                      className="btn"
                      disabled={probeLoading || activeLoading}
                      onClick={() => void runProbe({ refresh: true })}
                    >
                      {probeLoading ? "…" : "Перепроверить"}
                    </button>
                  </div>
                </div>

                <div className="probe-badges">
                  {badges.map((b) => (
                    <span key={b.id} className={`probe-badge is-${b.status}`}>
                      <span className="probe-glyph" aria-hidden="true">
                        {b.glyph}
                      </span>
                      <span>{b.label}</span>
                      {b.sub && <span className="probe-badge-sub mono">{b.sub}</span>}
                    </span>
                  ))}
                </div>

                {probe.notes.length > 0 && (
                  <details className="probe-fixes" open={probe.verdict !== "ok"}>
                    <summary>
                      {probe.verdict === "ok" ? `Подробности (${probe.notes.length})` : `Как исправить (${probe.notes.length})`}
                    </summary>
                    <ul>
                      {probe.notes.map((n, i) => (
                        <li key={i}>{n}</li>
                      ))}
                    </ul>
                  </details>
                )}
              </>
            ) : null}
          </div>
        )}

        {showVisionFieldOutside && visionModelRow}

        {(!isLocal || preset.id !== "custom" || !showVisionFieldOutside) && (
          <details className="more">
            <summary>{isLocal ? "Дополнительные настройки сервера" : "Адрес API и модель для экрана"}</summary>
            {(!isLocal || preset.id !== "custom") && apiUrlRow}
            {!isLocal && modelInputRow}
            {!showVisionFieldOutside && visionModelRow}
          </details>
        )}

        <Row label="Проверить подключение" hint="Короткий запрос к модели с текущими настройками.">
          <button className="btn" disabled={busy} onClick={onTest}>
            {busy ? "Проверяю…" : "Проверить"}
          </button>
        </Row>
      </div>
    </div>
  );
}
