// Settings → «Память» → «Чему я научился»: lessons JARVIS keeps about working
// with the user (their words for things, how the recogniser mishears them,
// preferences, humour), the «Учиться» switch and the review on demand.

import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { errorMessage } from "../lib/ipc";
import { hasTauri } from "../lib/workspace-ipc";
import { Group, Row, Switch } from "./controls";
import type { Settings } from "./SettingsApp";
import { groupLessons, kindTitle, lessonText, SAMPLE_LESSONS, SOURCE_LABEL, type Lesson } from "./lessonsModel";

type Tone = "ok" | "error" | "info";

interface Props {
  draft: Settings;
  set: (patch: (s: Settings) => Settings) => void;
  notify: (tone: Tone, text: string) => void;
  vy: boolean;
}

export function LessonsSection({ draft, set, notify, vy }: Props) {
  const [lessons, setLessons] = useState<Lesson[]>(hasTauri() ? [] : SAMPLE_LESSONS);
  const [reviewing, setReviewing] = useState(false);

  const load = useCallback(() => {
    if (!hasTauri()) return;
    void invoke<Lesson[]>("lessons_list").then(setLessons).catch(() => {});
  }, []);

  useEffect(() => {
    load();
    if (!hasTauri()) return;
    const off = listen("learn:changed", load);
    return () => {
      void off.then((f) => f());
    };
  }, [load]);

  const forget = (id: number) => {
    if (!hasTauri()) {
      setLessons((l) => l.filter((x) => x.id !== id));
      return;
    }
    void invoke("lesson_delete", { id }).then(load).catch((e) => notify("error", errorMessage(e)));
  };

  const review = async () => {
    if (!hasTauri()) {
      notify("info", "Разбор разговоров работает в приложении");
      return;
    }
    setReviewing(true);
    try {
      const n = await invoke<number>("learn_reflect_now");
      notify("ok", n > 0 ? `Готово: ${n} ${n === 1 ? "новый вывод" : n < 5 ? "новых вывода" : "новых выводов"}` : "Новых выводов нет — всё уже учтено");
      load();
    } catch (e) {
      notify("error", errorMessage(e));
    } finally {
      setReviewing(false);
    }
  };

  const groups = groupLessons(lessons);
  const on = draft.agent.selfLearning;

  return (
    <Group title="Чему я научился">
      <Row
        label="Учиться в работе"
        hint={`Запоминаю ${vy ? "ваши" : "твои"} слова для приложений, комнат и устройств, как ${vy ? "вас" : "тебя"} слышит распознавание, что ${vy ? "вам" : "тебе"} удобно и какие шутки заходят. Раз в день разбираю разговоры. Всё хранится только на этом компьютере.`}
      >
        <Switch
          label="Учиться в работе"
          checked={on}
          onChange={(v) => set((s) => ({ ...s, agent: { ...s.agent, selfLearning: v } }))}
        />
      </Row>
      {groups.length === 0 ? (
        <p className="empty">
          Пока ничего. {vy ? "Поправьте" : "Поправь"} меня, когда я ошибусь, — я запомню и в следующий раз пойму с полуслова.
        </p>
      ) : (
        groups.map((g) => (
          <div key={g.kind} className="lessons-group">
            <div className="expr-caption">{kindTitle(g.kind, vy)}</div>
            <ul className="memories">
              {g.items.map((l) => (
                <li key={l.id}>
                  <span>
                    {lessonText(l)}
                    <small className="lesson-meta">
                      {SOURCE_LABEL[l.source] ?? l.source}
                      {l.hits > 0 ? ` · пригодилось ${l.hits}×` : ""}
                    </small>
                  </span>
                  <button className="btn btn-danger" onClick={() => forget(l.id)}>
                    Забыть
                  </button>
                </li>
              ))}
            </ul>
          </div>
        ))
      )}
      <div className="actions">
        <button className="btn" disabled={!on || reviewing} onClick={() => void review()}>
          {reviewing ? "Разбираю…" : "Разобрать разговоры сейчас"}
        </button>
      </div>
    </Group>
  );
}
