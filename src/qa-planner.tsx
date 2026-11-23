// Visual QA for the planner window (`qa/planner.html`): the real
// PlannerApp with a sample week. `?view=day|4day|week|month`,
// `?pop=event|create`, `?plan=1` show the other states.

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "@fontsource-variable/onest";
import "./styles/tokens.css";
import "./styles/base.css";
import PlannerApp from "./planner/PlannerApp";
import { usePlanner, type CalEvent } from "./store/planner";
import { addDays, isoDate, startOfWeek } from "./planner/dates";
import type { TaskItem } from "./lib/types";

const q = new URLSearchParams(location.search);
const now = new Date();
now.setHours(14, 20, 0, 0);
const week = startOfWeek(now);
const day = (i: number) => isoDate(addDays(week, i));
let n = 1;
const ev = (d: number, start: string, end: string, title: string, category: string, extra: Partial<CalEvent> = {}): CalEvent => ({
  id: `qa-${n++}`,
  title,
  notes: "",
  location: "",
  start: start.length === 5 ? `${day(d)}T${start}` : start,
  end: end.length === 5 ? `${day(d)}T${end}` : end,
  allDay: false,
  color: "",
  category,
  done: false,
  seriesId: null,
  googleId: null,
  recurring: false,
  updatedAt: now.toISOString(),
  ...extra,
});

const events: CalEvent[] = [
  ev(0, "09:00", "10:30", "Глубокая работа: jarvis-island", "project"),
  ev(0, "11:00", "11:30", "Стендап", "work"),
  ev(0, "13:00", "13:20", "Английский: 20 минут", "study"),
  ev(0, "18:30", "19:30", "Зал: ноги", "sport"),
  ev(1, "08:00", "08:20", "Английский: 20 минут", "study", { done: true }),
  ev(1, "10:00", "12:00", "Ревью архитектуры планировщика", "work", { location: "Zoom" }),
  ev(1, "10:30", "11:15", "Созвон с Аней", "personal"),
  ev(1, "11:30", "12:30", "Обед", "personal", { color: "banana" }),
  ev(2, "09:30", "11:00", "Rust: владение и заимствование", "study"),
  ev(2, "15:00", "16:00", "Курс: system design", "study"),
  ev(2, "15:30", "16:30", "Встреча с заказчиком", "work", { color: "tomato" }),
  ev(2, "17:00", "17:45", "Пробежка 5 км", "sport"),
  ev(3, "07:30", "08:15", "Утренняя тренировка", "sport"),
  ev(3, "09:00", "12:00", "Глубокая работа: остров", "project"),
  ev(3, "14:00", "15:30", "Командный воркшоп", "work"),
  ev(4, "10:00", "11:00", "Демо пятницы", "work"),
  ev(4, "19:00", "22:00", "Кино с друзьями", "personal", { color: "lavender" }),
  ev(5, "11:00", "13:00", "Свой проект: Telegram-бот", "project"),
  ev(6, "10:00", "11:30", "Длинная прогулка", "sport", { color: "sage" }),
  ev(0, day(2), day(5), "Командировка в Алматы", "work", { allDay: true, color: "peacock" }),
  ev(4, day(4), day(5), "День рождения мамы", "personal", { allDay: true, color: "flamingo" }),
  ev(0, day(0), day(1), "Неделя без коротких видео", "study", { allDay: true }),
];

const tasks: TaskItem[] = [
  { id: "t1", title: "Оплатить интернет", notes: "", scheduledDate: day(0), scheduledTime: null, color: "blue", completed: false, completedAt: null, createdAt: now.toISOString() },
  { id: "t2", title: "Позвонить врачу", notes: "", scheduledDate: day(2), scheduledTime: "12:00", color: "blue", completed: true, completedAt: null, createdAt: now.toISOString() },
];

const view = (q.get("view") as "day" | "4day" | "week" | "month" | null) ?? "week";
usePlanner.setState({
  view,
  anchor: new Date(now.getFullYear(), now.getMonth(), now.getDate()),
  events,
  tasks,
  now,
  sync: { connected: true, account: "danila@gmail.com", clientId: "x", hasSecret: true, syncing: false, connecting: false, lastSync: new Date(now.getTime() - 2 * 60_000).toISOString(), error: null },
  suggestions:
    q.get("plan") === "1"
      ? [
          { title: "Глубокая работа: jarvis-island", start: `${isoDate(now)}T15:00`, end: `${isoDate(now)}T16:30`, category: "project", why: "" },
          { title: "Английский: 20 минут", start: `${isoDate(now)}T16:40`, end: `${isoDate(now)}T17:00`, category: "study", why: "" },
          { title: "Тренировка", start: `${isoDate(now)}T20:00`, end: `${isoDate(now)}T20:45`, category: "sport", why: "" },
        ]
      : [],
});

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <PlannerApp />
  </StrictMode>,
);
