// Smooths the running tool into what the face shows: see `activityStep`.

import { useEffect, useRef, useState } from "react";
import { activityStep, type Activity } from "../../store/emote";

export function useSteadyActivity(activity: Activity | null | undefined): Activity | null {
  const [shown, setShown] = useState<Activity | null>(activity ?? null);
  const shownRef = useRef(shown);
  const since = useRef(0);

  useEffect(() => {
    const target = activity ?? null;
    let timer: number | undefined;
    const tick = () => {
      const now = Date.now();
      const { show, wait } = activityStep(shownRef.current, since.current, target, now);
      if (show !== shownRef.current) {
        shownRef.current = show;
        since.current = now;
        setShown(show);
      }
      if (wait > 0) timer = window.setTimeout(tick, wait);
    };
    tick();
    return () => window.clearTimeout(timer);
  }, [activity]);

  return shown;
}
