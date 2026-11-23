// Growth worth a moment: a new level or a counted day. A few seconds, then
// the island folds back; a pointer on it opens «Рост».

import { daysRu, useMentor } from "../../store/mentor";
import { ART } from "../../mentor/art";

export function MomentCard() {
  const m = useMentor((s) => s.moment);
  const toNext = useMentor((s) => (s.state ? Math.max(0, s.state.levelTo - s.state.xpTotal) : null));
  if (!m) return null;
  const level = m.kind === "level";
  return (
    <div className={`view view-hello view-moment is-${m.kind}`} role="status">
      <div className="hello-main">
        <img className="moment-art" src={level ? ART.level : m.best ? ART.trophy : ART.streak} alt="" draggable={false} />
        <div className="hello-text">
          <span className="hello-title">{level ? `Новый уровень — ${m.name}` : m.best ? "Новый рекорд серии" : "День засчитан"}</span>
          <span className="hello-sub">
            {level ? `Уровень ${m.level}${toNext != null ? ` · до следующего ${toNext} XP` : ""}` : `Серия ${daysRu(m.streak)} подряд`}
          </span>
        </div>
        {m.xp > 0 && <span className="moment-xp">+{m.xp} XP</span>}
      </div>
    </div>
  );
}
