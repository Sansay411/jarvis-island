// A new version of JARVIS is out: one card per version, «Обновить» downloads,
// verifies, installs and restarts; «Позже» puts it away (Settings keeps it).

import { Icon } from "../../ui/Icon";
import { useUpdates } from "../../store/updates";
import { BatteryRing } from "./SoundDeck";

export function UpdateCard() {
  const status = useUpdates((s) => s.status);
  const progress = useUpdates((s) => s.progress);
  const error = useUpdates((s) => s.error);
  const install = useUpdates((s) => s.install);
  const dismiss = useUpdates((s) => s.dismiss);
  const next = status?.available;
  if (!next) return null;
  const pct = progress?.total ? Math.min(100, Math.round((progress.downloaded / progress.total) * 100)) : null;
  const busy = progress !== null;
  return (
    <div className="view view-hello view-update" role="status">
      <div className="hello-main">
        <BatteryRing level={busy ? (pct ?? 0) : 100} icon="bolt" size={50} stroke={3.5} />
        <div className="hello-text">
          <span className="hello-title">Джарвис {next.version}</span>
          <span className="hello-sub">
            {error ? error : busy ? (pct != null ? `Скачиваю обновление — ${pct}%` : "Скачиваю обновление…") : `Доступно обновление · сейчас ${next.current}`}
          </span>
        </div>
        {!busy && (
          <div className="hello-parts">
            <button type="button" className="hello-btn" onClick={dismiss}>
              Позже
            </button>
            <button type="button" className="hello-btn is-primary" onClick={() => void install()}>
              <Icon name="check" size={13} />
              Обновить
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
