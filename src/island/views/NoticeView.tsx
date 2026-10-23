// A timer ran out or a reminder is due: the island stays open with it until
// the user marks it done or puts it off.

import { Icon } from "../../ui/Icon";
import { localTime, reminderDone, reminderSnooze, useAlerts } from "../../store/reminders";

export function NoticeView() {
  const alerts = useAlerts();
  const r = alerts[0];
  if (!r) return null;
  const title = r.kind === "timer" ? (r.text ? `Таймер «${r.text}»` : "Таймер") : "Напоминание";
  const body = r.kind === "timer" ? `Время вышло · ${localTime(r.dueAt)}` : r.text;
  return (
    <div className="view view-notice">
      <div className="brow">
        <span className="brow-name notice-title">{title}</span>
        {alerts.length > 1 && <span className="notice-more">ещё {alerts.length - 1}</span>}
      </div>
      <div className="notice-body">
        <p className="notice-text">{body}</p>
        <div className="notice-actions">
          <button className="btn btn-quiet" onClick={() => void reminderSnooze(r.id, 5)}>Через 5 мин</button>
          <button className="btn btn-quiet" onClick={() => void reminderSnooze(r.id, 15)}>Через 15 мин</button>
          <button className="btn btn-solid" onClick={() => void reminderDone(r.id)}>
            <Icon name="check" size={14} /> Готово
          </button>
        </div>
      </div>
    </div>
  );
}
