// Settings -> «Навыки»: saved how-to playbooks, inline Markdown editor,
// install-from-URL preview & confirmation, and the «Учиться новым навыкам» switch.

import { AnimatePresence, motion } from "motion/react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { errorMessage } from "../lib/ipc";
import { hasTauri } from "../lib/workspace-ipc";
import { Icon } from "../ui/Icon";
import { Group, Row, Switch } from "./controls";
import type { Settings } from "./SettingsApp";
import {
  buildSamplePreview,
  filterSkills,
  formatBytesKb,
  formatLastUsed,
  formatUses,
  looksLikeSkillUrl,
  MAX_BODY_BYTES,
  SAMPLE_SKILLS,
  SOURCE_LABEL,
  utf8ByteLength,
  type SkillDetail,
  type SkillInstallPreview,
  type SkillSource,
  type SkillSummary,
} from "./skillsModel";

type Tone = "ok" | "error" | "info";

interface Props {
  draft: Settings;
  set: (patch: (s: Settings) => Settings) => void;
  notify: (tone: Tone, text: string) => void;
  vy: boolean;
}

interface EditorState {
  /** Existing slug when editing; null when creating a brand-new skill. */
  slug: string | null;
  name: string;
  description: string;
  body: string;
  source: SkillSource;
  sourceUrl: string | null;
  enabled: boolean;
}

function emptyEditor(): EditorState {
  return {
    slug: null,
    name: "",
    description: "",
    body: "## Цель\n\n## Шаги\n1. ",
    source: "user",
    sourceUrl: null,
    enabled: true,
  };
}

function editorFromDetail(d: SkillDetail): EditorState {
  return {
    slug: d.slug,
    name: d.name,
    description: d.description,
    body: d.body,
    source: d.source,
    sourceUrl: d.sourceUrl ?? null,
    enabled: d.enabled,
  };
}

export function SkillsSection({ draft, set, notify, vy }: Props) {
  const [skills, setSkills] = useState<SkillSummary[] | null>(null);
  const [fallbackDetails, setFallbackDetails] = useState<Record<string, SkillDetail>>(() =>
    Object.fromEntries(SAMPLE_SKILLS.map((s) => [s.slug, s])),
  );
  const [query, setQuery] = useState("");
  const [editor, setEditor] = useState<EditorState | null>(null);
  const [loadingSlug, setLoadingSlug] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [confirmDeleteSlug, setConfirmDeleteSlug] = useState<string | null>(null);

  // Install-from-URL state
  const [installUrl, setInstallUrl] = useState("");
  const [preview, setPreview] = useState<SkillInstallPreview | null>(null);
  const [previewBusy, setPreviewBusy] = useState(false);
  const [installBusy, setInstallBusy] = useState(false);

  const reduced = draft.reduceMotion;
  const collapseTransition = reduced
    ? { duration: 0 }
    : { type: "spring" as const, stiffness: 420, damping: 38 };

  const refresh = useCallback(() => {
    if (!hasTauri()) {
      setSkills((prev) => prev ?? SAMPLE_SKILLS.map(({ body: _b, ...rest }) => rest));
      return;
    }
    void invoke<SkillSummary[]>("skills_list")
      .then(setSkills)
      .catch((e) => {
        notify("error", errorMessage(e));
        setSkills([]);
      });
  }, [notify]);

  useEffect(() => {
    refresh();
    if (!hasTauri()) return;
    const un = listen("skills:changed", refresh);
    return () => void un.then((f) => f());
  }, [refresh]);

  const visible = useMemo(() => filterSkills(skills ?? [], query), [skills, query]);
  const bodyBytes = editor ? utf8ByteLength(editor.body) : 0;
  const bodyOverLimit = bodyBytes > MAX_BODY_BYTES;

  const openSkill = async (slug: string) => {
    if (editor?.slug === slug) {
      setEditor(null);
      setConfirmDeleteSlug(null);
      return;
    }
    setConfirmDeleteSlug(null);
    if (!hasTauri()) {
      const found = fallbackDetails[slug];
      if (found) setEditor(editorFromDetail(found));
      return;
    }
    setLoadingSlug(slug);
    try {
      const detail = await invoke<SkillDetail>("skills_read", { slug });
      setEditor(editorFromDetail(detail));
    } catch (e) {
      notify("error", errorMessage(e));
    } finally {
      setLoadingSlug(null);
    }
  };

  const toggleEnabled = async (s: SkillSummary, enabled: boolean) => {
    if (!hasTauri()) {
      setSkills((prev) => (prev ?? []).map((x) => (x.slug === s.slug ? { ...x, enabled } : x)));
      setFallbackDetails((prev) => {
        const existing = prev[s.slug];
        return existing ? { ...prev, [s.slug]: { ...existing, enabled } } : prev;
      });
      if (editor?.slug === s.slug) setEditor({ ...editor, enabled });
      return;
    }
    try {
      await invoke("skills_set_enabled", { slug: s.slug, enabled });
      setSkills((prev) => (prev ?? []).map((x) => (x.slug === s.slug ? { ...x, enabled } : x)));
      if (editor?.slug === s.slug) setEditor({ ...editor, enabled });
    } catch (e) {
      notify("error", errorMessage(e));
    }
  };

  const saveEditor = async () => {
    if (!editor) return;
    const name = editor.name.trim();
    const description = editor.description.trim();
    const body = editor.body.trim();
    if (!name || !body || bodyOverLimit) return;

    setSaving(true);
    try {
      if (!hasTauri()) {
        const slug =
          editor.slug ??
          (name
            .toLowerCase()
            .replace(/[^a-z0-9а-яё]+/gi, "-")
            .replace(/^-|-$/g, "") || `skill-${Date.now()}`);
        const prev = fallbackDetails[slug];
        const detail: SkillDetail = {
          slug,
          name,
          description,
          body,
          created: prev?.created ?? new Date().toISOString(),
          source: prev?.source ?? editor.source,
          sourceUrl: prev?.sourceUrl ?? editor.sourceUrl,
          enabled: editor.enabled,
          uses: prev?.uses ?? 0,
          lastUsed: prev?.lastUsed ?? null,
        };
        setFallbackDetails((all) => ({ ...all, [slug]: detail }));
        const { body: _b, ...summary } = detail;
        setSkills((all) => {
          const list = all ?? [];
          return list.some((x) => x.slug === slug)
            ? list.map((x) => (x.slug === slug ? summary : x))
            : [summary, ...list];
        });
        setEditor(null);
        notify("ok", `Навык «${name}» сохранён`);
        return;
      }

      const saved = await invoke<SkillDetail>("skills_save", {
        input: {
          slug: editor.slug,
          name,
          description,
          body,
          enabled: editor.enabled,
          source: editor.slug ? undefined : editor.source,
          sourceUrl: editor.sourceUrl,
        },
      });
      setEditor(null);
      notify("ok", `Навык «${saved.name}» сохранён`);
      refresh();
    } catch (e) {
      notify("error", errorMessage(e));
    } finally {
      setSaving(false);
    }
  };

  const deleteSkill = async (s: SkillSummary) => {
    setConfirmDeleteSlug(null);
    if (!hasTauri()) {
      setSkills((prev) => (prev ?? []).filter((x) => x.slug !== s.slug));
      setFallbackDetails((prev) => {
        const next = { ...prev };
        delete next[s.slug];
        return next;
      });
      if (editor?.slug === s.slug) setEditor(null);
      notify("ok", `Навык «${s.name}» удалён`);
      return;
    }
    try {
      await invoke("skills_delete", { slug: s.slug });
      if (editor?.slug === s.slug) setEditor(null);
      notify("ok", `Навык «${s.name}» удалён`);
      refresh();
    } catch (e) {
      notify("error", errorMessage(e));
    }
  };

  const fetchPreview = async () => {
    const url = installUrl.trim();
    if (!looksLikeSkillUrl(url)) {
      notify("error", "Укажите публичную HTTP(S)-ссылку на файл SKILL.md или папку GitHub");
      return;
    }
    setPreviewBusy(true);
    setPreview(null);
    try {
      if (!hasTauri()) {
        const existing = new Set((skills ?? []).map((s) => s.slug));
        setPreview(buildSamplePreview(url, existing));
        return;
      }
      const p = await invoke<SkillInstallPreview>("skills_install_preview", { url });
      setPreview(p);
    } catch (e) {
      notify("error", errorMessage(e));
    } finally {
      setPreviewBusy(false);
    }
  };

  const confirmInstall = async () => {
    if (!preview) return;
    setInstallBusy(true);
    try {
      if (!hasTauri()) {
        const detail: SkillDetail = {
          slug: preview.slug,
          name: preview.name,
          description: preview.description,
          body: preview.body,
          created: new Date().toISOString(),
          source: "installed",
          sourceUrl: preview.rawUrl,
          enabled: true,
          uses: 0,
          lastUsed: null,
        };
        setFallbackDetails((all) => ({ ...all, [detail.slug]: detail }));
        const { body: _b, ...summary } = detail;
        setSkills((all) => {
          const list = all ?? [];
          return list.some((x) => x.slug === summary.slug)
            ? list.map((x) => (x.slug === summary.slug ? summary : x))
            : [summary, ...list];
        });
        notify("ok", `Навык «${preview.name}» установлен`);
        setPreview(null);
        setInstallUrl("");
        return;
      }
      const installed = await invoke<SkillSummary>("skills_install", { url: preview.url });
      notify("ok", `Навык «${installed.name}» установлен`);
      setPreview(null);
      setInstallUrl("");
      refresh();
    } catch (e) {
      notify("error", errorMessage(e));
    } finally {
      setInstallBusy(false);
    }
  };

  const renderEditorForm = (state: EditorState, isNew: boolean) => (
    <div className="skill-editor">
      {isNew && <div className="skill-editor-head">Новый навык</div>}
      <Row label="Название" hint="Короткое имя — по нему Джарвис узнаёт навык.">
        <input
          value={state.name}
          maxLength={120}
          placeholder="Например, Выгрузка отчёта за неделю"
          onChange={(e) => setEditor({ ...state, name: e.target.value })}
        />
      </Row>
      <Row label="Когда применять" hint="Кратко опишите ситуацию, при которой нужен этот навык.">
        <input
          value={state.description}
          maxLength={400}
          placeholder="Когда прошу собрать еженедельный отчёт из папки Загрузки…"
          onChange={(e) => setEditor({ ...state, description: e.target.value })}
        />
      </Row>
      <div className="skill-md-field">
        <div className="skill-md-label-row">
          <label className="row-label" htmlFor={`skill-body-${state.slug ?? "new"}`}>
            Инструкция (Markdown)
          </label>
          <span className={`skill-size mono${bodyOverLimit ? " is-over" : ""}`}>
            {formatBytesKb(bodyBytes)} / 16 КБ
          </span>
        </div>
        <div className="row-hint">Цель, шаги по порядку, нужные инструменты и важные нюансы.</div>
        <textarea
          id={`skill-body-${state.slug ?? "new"}`}
          className="mono skill-md-textarea"
          rows={10}
          spellCheck={false}
          value={state.body}
          onChange={(e) => setEditor({ ...state, body: e.target.value })}
        />
      </div>
      {state.source === "installed" && state.sourceUrl && (
        <p className="note skill-source-note">
          Источник: <span className="mono">{state.sourceUrl}</span>. Установленный навык работает только как текстовая инструкция и не даёт новых прав.
        </p>
      )}
      <div className="skill-editor-actions">
        <button type="button" className="btn" disabled={saving} onClick={() => setEditor(null)}>
          Отмена
        </button>
        <button
          type="button"
          className="btn btn-primary"
          disabled={saving || !state.name.trim() || !state.body.trim() || bodyOverLimit}
          onClick={() => void saveEditor()}
        >
          {saving ? "Сохраняю…" : "Сохранить"}
        </button>
      </div>
    </div>
  );

  if (skills === null) return <p className="empty">Загружаю…</p>;

  return (
    <>
      <Group title="Обучение">
        <Row
          label="Учиться новым навыкам"
          hint="После сложной многошаговой задачи Джарвис коротко предложит сохранить способ как навык. Без вашего согласия ничего не сохраняется."
        >
          <Switch
            label="Учиться новым навыкам"
            checked={draft.agent.learnSkills}
            onChange={(v) => set((s) => ({ ...s, agent: { ...s.agent, learnSkills: v } }))}
          />
        </Row>
      </Group>

      <Group title={`Сохранённые навыки${skills.length > 0 ? ` · ${skills.length}` : ""}`}>
        <div className="skills-toolbar">
          {skills.length >= 3 && (
            <input
              className="skills-search"
              value={query}
              placeholder="Найти навык по названию или описанию…"
              aria-label="Поиск навыков"
              onChange={(e) => setQuery(e.target.value)}
            />
          )}
          <button
            type="button"
            className="btn"
            onClick={() => {
              setConfirmDeleteSlug(null);
              setEditor((cur) => (cur && cur.slug === null ? null : emptyEditor()));
            }}
          >
            <Icon name="plus" size={14} />
            <span>Новый навык</span>
          </button>
        </div>

        <AnimatePresence initial={false}>
          {editor && editor.slug === null && (
            <motion.div
              className="skill-new-wrap"
              initial={{ height: 0, opacity: 0 }}
              animate={{ height: "auto", opacity: 1 }}
              exit={{ height: 0, opacity: 0 }}
              transition={collapseTransition}
            >
              {renderEditorForm(editor, true)}
            </motion.div>
          )}
        </AnimatePresence>

        {skills.length === 0 ? (
          <p className="empty">
            Пока ни одного навыка. {vy ? "Скажите" : "Скажи"} «запомни, как ты это сделал» после задачи — или установите готовый по ссылке ниже.
          </p>
        ) : visible.length === 0 ? (
          <p className="empty">По запросу «{query}» ничего не нашлось.</p>
        ) : (
          <ul className="skills-list">
            {visible.map((s) => {
              const isOpen = editor?.slug === s.slug;
              const isConfirmingDelete = confirmDeleteSlug === s.slug;
              const last = formatLastUsed(s.lastUsed);
              return (
                <li
                  key={s.slug}
                  className={`skill-item${s.enabled ? "" : " is-disabled"}${isOpen ? " is-open" : ""}`}
                >
                  <div className="skill-row">
                    <button
                      type="button"
                      className="skill-main"
                      aria-expanded={isOpen}
                      onClick={() => void openSkill(s.slug)}
                    >
                      <div className="skill-top">
                        <span className="skill-name">{s.name}</span>
                        <span className={`skill-badge is-${s.source}`}>{SOURCE_LABEL[s.source]}</span>
                        <span className="skill-meta">
                          {formatUses(s.uses)}
                          {last ? ` · ${last}` : ""}
                        </span>
                      </div>
                      {s.description && <div className="skill-desc">{s.description}</div>}
                    </button>

                    <div className="skill-controls">
                      {isConfirmingDelete ? (
                        <div className="skill-confirm-del">
                          <button
                            type="button"
                            className="btn btn-danger"
                            onClick={() => void deleteSkill(s)}
                          >
                            Удалить?
                          </button>
                          <button
                            type="button"
                            className="btn"
                            onClick={() => setConfirmDeleteSlug(null)}
                          >
                            Отмена
                          </button>
                        </div>
                      ) : (
                        <>
                          <button
                            type="button"
                            className="btn"
                            disabled={loadingSlug === s.slug}
                            onClick={() => void openSkill(s.slug)}
                          >
                            {loadingSlug === s.slug ? "…" : isOpen ? "Свернуть" : "Открыть"}
                          </button>
                          <button
                            type="button"
                            className="btn btn-danger"
                            aria-label={`Удалить навык ${s.name}`}
                            onClick={() => setConfirmDeleteSlug(s.slug)}
                          >
                            Удалить
                          </button>
                        </>
                      )}
                      <Switch
                        label={`Навык ${s.name}`}
                        checked={s.enabled}
                        onChange={(v) => void toggleEnabled(s, v)}
                      />
                    </div>
                  </div>

                  <AnimatePresence initial={false}>
                    {isOpen && editor && (
                      <motion.div
                        className="skill-drawer"
                        initial={{ height: 0, opacity: 0 }}
                        animate={{ height: "auto", opacity: 1 }}
                        exit={{ height: 0, opacity: 0 }}
                        transition={collapseTransition}
                      >
                        {renderEditorForm(editor, false)}
                      </motion.div>
                    )}
                  </AnimatePresence>
                </li>
              );
            })}
          </ul>
        )}
      </Group>

      <Group title="Установить по ссылке">
        <p className="note skill-install-note">
          Поддерживаются прямые ссылки на <code>SKILL.md</code> и ссылки GitHub на файл, папку или репозиторий. Сначала откроется предпросмотр. Установленные навыки — только текстовые инструкции, новых разрешений они не дают.
        </p>
        <div className="skill-install-bar">
          <input
            className="mono"
            value={installUrl}
            spellCheck={false}
            placeholder="https://github.com/owner/repo/blob/main/skills/report/SKILL.md"
            aria-label="Ссылка на файл SKILL.md"
            onChange={(e) => {
              setInstallUrl(e.target.value);
              if (preview) setPreview(null);
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter" && installUrl.trim() && !previewBusy) void fetchPreview();
            }}
          />
          <button
            type="button"
            className="btn"
            disabled={previewBusy || !installUrl.trim()}
            onClick={() => void fetchPreview()}
          >
            {previewBusy ? "Загружаю…" : "Предпросмотр"}
          </button>
        </div>

        <AnimatePresence initial={false}>
          {preview && (
            <motion.div
              className="skill-preview"
              initial={{ height: 0, opacity: 0 }}
              animate={{ height: "auto", opacity: 1 }}
              exit={{ height: 0, opacity: 0 }}
              transition={collapseTransition}
            >
              <div className="skill-preview-card">
                <div className="skill-preview-head">
                  <div className="skill-preview-title-wrap">
                    <div className="skill-top">
                      <span className="skill-name">{preview.name}</span>
                      <span className="skill-badge is-installed">Предпросмотр</span>
                      <span className="skill-meta mono">{formatBytesKb(preview.sizeBytes)}</span>
                    </div>
                    {preview.description && <div className="skill-desc">{preview.description}</div>}
                    <div className="skill-preview-url mono">{preview.rawUrl}</div>
                  </div>
                </div>

                {preview.exists && (
                  <div className="callout skill-preview-warn">
                    Навык с идентификатором <span className="mono">{preview.slug}</span> уже существует и будет обновлён.
                  </div>
                )}

                <pre className="skill-preview-body mono">{preview.body}</pre>

                <div className="skill-preview-foot">
                  <span className="skill-preview-safe">
                    Проверьте шаги перед установкой. Навык не отменяет правила безопасности и подтверждения.
                  </span>
                  <div className="skill-preview-actions">
                    <button
                      type="button"
                      className="btn"
                      disabled={installBusy}
                      onClick={() => setPreview(null)}
                    >
                      Отмена
                    </button>
                    <button
                      type="button"
                      className="btn btn-primary"
                      disabled={installBusy}
                      onClick={() => void confirmInstall()}
                    >
                      {installBusy ? "Устанавливаю…" : "Установить навык"}
                    </button>
                  </div>
                </div>
              </div>
            </motion.div>
          )}
        </AnimatePresence>
      </Group>
    </>
  );
}
