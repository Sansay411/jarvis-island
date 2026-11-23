// Settings for what was added in 0.3: «Наставник» (short-video budget,
// strict hours, what the user grows in, the if-then plan), «Календарь»
// (working day and Google Calendar sync with the user's own OAuth client)
// and «Telegram» (the user's own bot: token, binding, schedule).

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { errorMessage } from "../lib/ipc";
import { Group, Row, Switch } from "./controls";
import type { Settings } from "./SettingsApp";

type Tone = "ok" | "error" | "info";
type SetFn = (patch: (s: Settings) => Settings) => void;
interface Props {
  draft: Settings;
  set: SetFn;
  notify: (tone: Tone, text: string) => void;
}

const TRACKS: Array<{ id: string; label: string; hint: string }> = [
  { id: "code", label: "Программирование", hint: "Вопросы и «что выведет код» в ленте, игра на счёт." },
  { id: "english", label: "Английский", hint: "Слова с интервальным повторением, игра на скорость." },
  { id: "body", label: "Спорт и тело", hint: "Разминки на 3 минуты, дыхание, тренировка в плане дня." },
  { id: "projects", label: "Свои проекты", hint: "Шаги по папкам проектов и глубокая работа утром." },
];

function num(v: string, min: number, max: number): number {
  const n = Math.round(Number(v));
  return Number.isFinite(n) ? Math.max(min, Math.min(max, n)) : min;
}

export function MentorSection({ draft, set }: Props) {
  const m = draft.mentor;
  const put = (patch: Partial<Settings["mentor"]>) => set((s) => ({ ...s, mentor: { ...s.mentor, ...patch } }));
  return (
    <>
      <Group title="Короткие видео">
        <Row label="Следить за короткими видео" hint="TikTok, YouTube Shorts, Reels и VK Клипы в браузере и приложении TikTok. Когда минуты кончаются, Джарвис закрывает вкладку и предлагает замену.">
          <Switch label="Следить за короткими видео" checked={m.guard} onChange={(v) => put({ guard: v })} />
        </Row>
        <Row label="Минут коротких видео в день">
          <input type="number" min={0} max={240} value={m.shortMinutes} onChange={(e) => put({ shortMinutes: num(e.target.value, 0, 240) })} />
        </Row>
        <Row label="Минут YouTube в день" hint="Учебные ролики не считаются: по словам в названии или после «Это была учёба».">
          <input type="number" min={0} max={480} value={m.youtubeMinutes} onChange={(e) => put({ youtubeMinutes: num(e.target.value, 0, 480) })} />
        </Row>
        <Row label="Польза добавляет минуты" hint="Лента, игра, разминка и фокус дают ещё до 20 минут в день: сначала полезное, потом приятное.">
          <Switch label="Польза добавляет минуты" checked={m.earn} onChange={(v) => put({ earn: v })} />
        </Row>
        <Row label="Строго во время фокуса" hint="Пока идёт фокус или блок «Работа», «Учёба», «Проекты» в календаре, короткие видео закрываются сразу.">
          <Switch label="Строго во время фокуса" checked={m.strictFocus} onChange={(v) => put({ strictFocus: v })} />
        </Row>
        <Row label="Тихие часы" hint="Ночью лента крадёт сон. В эти часы короткие видео закрываются сразу. Пусто — выключено.">
          <span className="row-pair">
            <input type="time" value={m.nightFrom} onChange={(e) => put({ nightFrom: e.target.value })} aria-label="С" />
            <span>—</span>
            <input type="time" value={m.nightTo} onChange={(e) => put({ nightTo: e.target.value })} aria-label="До" />
          </span>
        </Row>
        <Row label="Говорить вслух" hint="«TikTok уже десять минут. Я закрыл…» — голосом Джарвиса.">
          <Switch label="Говорить вслух" checked={m.speak} onChange={(v) => put({ speak: v })} />
        </Row>
      </Group>

      <Group title="Что я развиваю">
        {TRACKS.map((t) => (
          <Row key={t.id} label={t.label} hint={t.hint}>
            <Switch
              label={t.label}
              checked={m.tracks.includes(t.id)}
              onChange={(v) => put({ tracks: v ? [...m.tracks.filter((x) => x !== t.id), t.id] : m.tracks.filter((x) => x !== t.id) })}
            />
          </Row>
        ))}
      </Group>

      <Group title="Мой план на момент тяги">
        <Row label="Если потянет в ленту, то я…" hint="Свой план «если — то» срабатывает намного чаще простого намерения. Джарвис покажет его, когда закроет ленту." wide>
          <input value={m.ifThen} placeholder="сделаю 20 приседаний и одну карточку английского" onChange={(e) => put({ ifThen: e.target.value })} />
        </Row>
        <Row label="Слова учебных видео" hint="Через запятую. Ролик с таким словом в названии не тратит минуты YouTube." wide>
          <textarea rows={3} value={m.learningWords.join(", ")} onChange={(e) => put({ learningWords: e.target.value.split(",").map((w) => w.trim()).filter(Boolean) })} />
        </Row>
      </Group>
    </>
  );
}

interface SyncStatus {
  connected: boolean;
  account: string;
  clientId: string;
  hasSecret: boolean;
  syncing: boolean;
  connecting: boolean;
  lastSync: string | null;
  error: string | null;
}

export function CalendarSection({ draft, set, notify }: Props) {
  const p = draft.planner;
  const put = (patch: Partial<Settings["planner"]>) => set((s) => ({ ...s, planner: { ...s.planner, ...patch } }));
  const [st, setSt] = useState<SyncStatus | null>(null);
  const [clientId, setClientId] = useState("");
  const [secret, setSecret] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    invoke<SyncStatus>("planner_google_status")
      .then((s) => {
        setSt(s);
        setClientId(s.clientId);
      })
      .catch(() => {});
  }, []);
  const run = async (cmd: string, args: Record<string, unknown> = {}, ok?: string) => {
    setBusy(true);
    try {
      const s = await invoke<SyncStatus>(cmd, args);
      setSt(s);
      if (ok) notify("ok", ok);
    } catch (e) {
      notify("error", errorMessage(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <>
      <Group title="Планировщик">
        <Row label="Рабочий день" hint="«Спланировать день» ставит глубокую работу и учёбу внутрь него.">
          <span className="row-pair">
            <input type="time" value={p.workStart} onChange={(e) => put({ workStart: e.target.value })} aria-label="Начало" />
            <span>—</span>
            <input type="time" value={p.workEnd} onChange={(e) => put({ workEnd: e.target.value })} aria-label="Конец" />
          </span>
        </Row>
        <Row label="Окно календаря" hint="Как в Google: неделя, день, месяц, перетаскивание и растягивание событий. Клавиши T, D, W, M, C.">
          <button className="btn" onClick={() => void invoke("planner_open", { date: null })}>
            Открыть планировщик
          </button>
        </Row>
      </Group>

      <Group title="Google Календарь">
        <Row
          label={st?.connected ? `Подключён${st.account ? `: ${st.account}` : ""}` : "Не подключён"}
          hint={
            st?.error
              ? st.error
              : st?.connected
                ? st.syncing
                  ? "Синхронизация…"
                  : st.lastSync
                    ? `Синхронизирован ${new Date(st.lastSync).toLocaleString("ru-RU")}. События ходят в обе стороны: появятся и в Google Календаре на телефоне.`
                    : "Первая синхронизация скоро начнётся."
                : "Свой ключ Google: Джарвис не ходит через чужие серверы."
          }
        >
          {st?.connected ? (
            <span className="row-pair">
              <button className="btn" disabled={busy} onClick={() => void run("planner_google_sync", {}, "Синхронизировано")}>
                Синхронизировать
              </button>
              <button className="btn" disabled={busy} onClick={() => void run("planner_google_disconnect", {}, "Google Календарь отключён")}>
                Отключить
              </button>
            </span>
          ) : null}
        </Row>
        {!st?.connected && (
          <>
            <Row label="Client ID" wide>
              <input className="mono" value={clientId} spellCheck={false} placeholder="123…apps.googleusercontent.com" onChange={(e) => setClientId(e.target.value.trim())} />
            </Row>
            <Row label="Client secret" hint={st?.hasSecret ? "Сохранён в хранилище ключей Windows. Оставьте пустым, чтобы не менять." : undefined} wide>
              <input className="mono" type="password" value={secret} spellCheck={false} placeholder={st?.hasSecret ? "••••••••" : "GOCSPX-…"} onChange={(e) => setSecret(e.target.value.trim())} />
            </Row>
            <Row label="Войти через Google" hint={st?.connecting ? "Откройте браузер и подтвердите доступ…" : "Откроется браузер: выберите аккаунт и разрешите доступ к календарю."}>
              <button className="btn btn-solid" disabled={busy || !clientId} onClick={() => void run("planner_google_connect", { clientId, clientSecret: secret }, "Google Календарь подключён")}>
                {busy ? "Жду браузер…" : "Подключить"}
              </button>
            </Row>
            <div className="howto">
              <b>Как получить ключ (один раз, 5 минут)</b>
              <ol>
                <li>
                  Откройте <span className="mono">console.cloud.google.com</span> и создайте проект.
                </li>
                <li>«APIs & Services» → «Library» → включите <b>Google Calendar API</b>.</li>
                <li>«OAuth consent screen»: тип External, добавьте свою почту в Test users. Чтобы доступ не истекал через 7 дней, нажмите «Publish app».</li>
                <li>«Credentials» → «Create credentials» → «OAuth client ID» → тип <b>Desktop app</b>.</li>
                <li>Скопируйте Client ID и Client secret сюда и нажмите «Подключить».</li>
              </ol>
            </div>
          </>
        )}
      </Group>
    </>
  );
}

interface BotStatus {
  hasToken: boolean;
  username: string;
  bound: boolean;
  running: boolean;
  error: string | null;
  link: string | null;
}

export function TelegramSection({ draft, set, notify }: Props) {
  const t = draft.telegram;
  const put = (patch: Partial<Settings["telegram"]>) => set((s) => ({ ...s, telegram: { ...s.telegram, ...patch } }));
  const [st, setSt] = useState<BotStatus | null>(null);
  const [token, setToken] = useState("");
  const refresh = () => void invoke<BotStatus>("telegram_status").then(setSt).catch(() => {});
  useEffect(() => {
    refresh();
    const id = window.setInterval(refresh, 4000);
    return () => window.clearInterval(id);
  }, []);
  const run = async (cmd: string, args: Record<string, unknown> = {}, ok?: string) => {
    try {
      setSt(await invoke<BotStatus>(cmd, args));
      if (ok) notify("ok", ok);
    } catch (e) {
      notify("error", errorMessage(e));
    }
  };
  return (
    <>
      <Group title="Бот">
        <Row
          label={st?.hasToken ? (st.username ? `@${st.username}` : "Токен сохранён") : "Бот не настроен"}
          hint={
            st?.error ??
            (st?.hasToken
              ? st.bound
                ? "Чат привязан. Бот работает, пока включён компьютер; сообщения за это время Telegram хранит сутки."
                : "Осталось привязать свой чат."
              : "Создайте бота у @BotFather в Telegram (/newbot) и вставьте токен.")
          }
        >
          {st?.hasToken && (
            <button className="btn" onClick={() => void run("telegram_set_token", { token: "" }, "Токен удалён")}>
              Удалить токен
            </button>
          )}
        </Row>
        {!st?.hasToken && (
          <Row label="Токен от @BotFather" hint="Хранится в диспетчере учётных данных Windows." wide>
            <span className="row-pair is-grow">
              <input className="mono" type="password" value={token} spellCheck={false} placeholder="123456789:AA…" onChange={(e) => setToken(e.target.value.trim())} />
              <button className="btn btn-solid" disabled={!token} onClick={() => void run("telegram_set_token", { token }, "Токен сохранён").then(() => setToken(""))}>
                Сохранить
              </button>
            </span>
          </Row>
        )}
        {st?.hasToken && (
          <Row label={st.bound ? "Свой чат привязан" : "Привязать свой чат"} hint={st.link ? "Откройте ссылку на телефоне и нажмите «Start». Ссылка одноразовая." : "Бот отвечает только одному чату — вашему."}>
            {st.bound ? (
              <button className="btn" onClick={() => void run("telegram_unbind", {}, "Чат отвязан")}>
                Отвязать
              </button>
            ) : st.link ? (
              <span className="mono selectable">{st.link}</span>
            ) : (
              <button className="btn btn-solid" onClick={() => void run("telegram_pair")}>
                Получить ссылку
              </button>
            )}
          </Row>
        )}
      </Group>
      <Group title="Что бот делает сам">
        <Row label="Бот включён">
          <Switch label="Бот включён" checked={t.enabled} onChange={(v) => put({ enabled: v })} />
        </Row>
        <Row label="Утренний план" hint="План дня, задачи и твой план «если — то». Пусто — не присылать.">
          <input type="time" value={t.morning} onChange={(e) => put({ morning: e.target.value })} />
        </Row>
        <Row label="Вечерний итог" hint="Статистика дня и итог из трёх вопросов.">
          <input type="time" value={t.evening} onChange={(e) => put({ evening: e.target.value })} />
        </Row>
        <Row label="Напоминать о событии за, мин" hint="0 — не напоминать. С кнопкой «Начать фокус на компьютере».">
          <input type="number" min={0} max={120} value={t.remindBefore} onChange={(e) => put({ remindBefore: num(e.target.value, 0, 120) })} />
        </Row>
        <Row label="Просьбы Джарвису с телефона" hint="Обычное сообщение боту — задача Джарвису на компьютере. Всё, что что-то меняет, ждёт подтверждения на компьютере.">
          <Switch label="Просьбы с телефона" checked={t.agent} onChange={(v) => put({ agent: v })} />
        </Row>
      </Group>
    </>
  );
}
