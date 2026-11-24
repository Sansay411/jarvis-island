// The opened island: four tabs (Обзор, Фокус, Рост, Полка; Звук and the year map open from inside) under a
// brow whose centre column belongs to JARVIS's eyes. The tabs are the only
// deck switcher (plus Alt+1..5 and a horizontal swipe). The pin keeps the
// island open.

import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { DECK_SPRING, deckFadeVariants, deckVariants } from "../motion";
import { useEffect, useRef, useState, type WheelEvent } from "react";
import { Icon, type IconName } from "../../ui/Icon";
import { ipc, isTauri } from "../../lib/ipc";
import { hasActiveMedia, useSystem } from "../../store/system";
import { useIsland } from "../../store/island";
import { useHub } from "../../store/hub";
import { useTts } from "../../store/tts";
import { toggleVoice, useVoice } from "../../store/voice";
import type { PeekDeck } from "../geometry";
import { EqualizerBars, IdlePlayer, MediaCard } from "./MediaCard";
import { JobsStrip } from "./JobsStrip";
import { ClocksStrip } from "./ClocksStrip";
import { WeekCalendar } from "./WeekCalendar";
import { FocusDeck, FocusHoverBanner } from "./FocusDeck";
import { TrayDeck } from "./TrayDeck";
import { ActivityDeck } from "./ActivityDeck";
import { AgentMissionBar } from "./AgentMissionBar";
import { HeadsetChip, SoundDeck } from "./SoundDeck";
import { GrowthDeck } from "./GrowthDeck";
import { invoke } from "@tauri-apps/api/core";
import { deg, useWeather } from "../../store/weather";
import { WeatherGlyph } from "../../ui/WeatherGlyph";

// Four tabs keep the brow clear of the eyes. «Звук» opens from the headset
// chip, the year map from «Рост».
const DECKS: Array<{ id: PeekDeck; label: string; icon: IconName }> = [
  { id: "nook", label: "Обзор", icon: "nook" },
  { id: "focus", label: "Фокус", icon: "target" },
  { id: "growth", label: "Рост", icon: "grid" },
  { id: "tray", label: "Полка", icon: "tray" },
];

export function PeekView() {
  const media = useSystem((s) => s.media);
  const withMedia = hasActiveMedia(media);
  const peekDeck = useIsland((s) => s.peekDeck);
  const focusBannerDismissed = useIsland((s) => s.focusBannerDismissed);
  const forceFocusBanner = useIsland((s) => s.forceFocusBanner);
  const setIsland = useIsland((s) => s.set);
  const setDeck = useIsland((s) => s.setDeck);
  const deckDir = useIsland((s) => s.deckDir);
  const pinned = useIsland((s) => s.pinned);
  const togglePinned = useIsland((s) => s.togglePinned);
  const reduced = useReducedMotion();

  const focus = useHub((s) => s.focus);
  const shelfCount = useHub((s) => s.shelf.length);

  const ttsSpeaking = useTts((s) => s.speaking);
  const stopTts = useTts((s) => s.stop);

  const showFocusBanner = forceFocusBanner || (focus.active && !focusBannerDismissed);

  // Alt+1..5 switches decks rapidly.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!e.altKey) return;
      const idx = Number(e.key) - 1;
      if (idx >= 0 && idx < DECKS.length) {
        e.preventDefault();
        setDeck(DECKS[idx]!.id);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [setDeck]);

  // Horizontal trackpad swipe switches decks with debounce.
  const wheelLock = useRef(0);
  const onWheel = (e: WheelEvent<HTMLDivElement>) => {
    if (Math.abs(e.deltaX) < 28 || Math.abs(e.deltaX) <= Math.abs(e.deltaY)) return;
    const now = performance.now();
    if (now - wheelLock.current < 320) return;
    wheelLock.current = now;
    const curIdx = DECKS.findIndex((d) => d.id === peekDeck);
    const nextIdx = e.deltaX > 0 ? Math.min(DECKS.length - 1, curIdx + 1) : Math.max(0, curIdx - 1);
    if (nextIdx !== curIdx) setDeck(DECKS[nextIdx]!.id);
  };

  // The cover lights the whole opened island from the left — behind the tabs
  // too — and fades out before the calendar. Only on `Обзор` with music.
  const art = useSystem((s) =>
    media && s.artwork && s.artwork.trackKey === media.trackKey ? s.artwork.dataUrl : null,
  );
  const ambient = withMedia && art && !showFocusBanner && peekDeck === "nook";

  return (
    <div className="view view-peek" onWheel={onWheel}>
      <AnimatePresence>
        {ambient && (
          <motion.div
            key="ambient"
            className={`peek-ambient${media?.status === "playing" ? " is-playing" : ""}`}
            aria-hidden="true"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.6, ease: [0.22, 1, 0.36, 1] }}
          >
            <AnimatePresence initial={false}>
              <motion.img
                key={media?.trackKey}
                src={art}
                alt=""
                draggable={false}
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                transition={{ duration: 0.9 }}
              />
            </AnimatePresence>
            <span className="peek-ambient-pool" />
            <span className="peek-ambient-veil" />
          </motion.div>
        )}
      </AnimatePresence>
      {/* Top Brow: 3-column grid reserving an untouchable 72px center slot for Eyes.tsx */}
      <div className="brow brow-split">
        <div className="brow-left" role="tablist" aria-label="Разделы острова">
          {DECKS.map((d) => {
            const owner = peekDeck === "activity" ? "growth" : peekDeck;
            const active = !showFocusBanner && owner === d.id;
            return (
              <button
                key={d.id}
                type="button"
                role="tab"
                aria-selected={active}
                className={`deck-tab${active ? " is-active" : ""}`}
                onClick={() => setDeck(d.id)}
              >
                {active && (
                  <motion.span
                    layoutId="peek-deck-pill"
                    className="deck-tab-bg"
                    transition={{ type: "spring", stiffness: 520, damping: 36 }}
                  />
                )}
                <span className="deck-tab-inner">
                  <span>{d.label}</span>
                  {d.id === "focus" && focus.active && (
                    <span className={`deck-tab-dot${focus.paused ? " is-paused" : ""}`} aria-hidden="true" />
                  )}
                  {d.id === "tray" && shelfCount > 0 && <span className="deck-tab-badge">{shelfCount}</span>}
                </span>
              </button>
            );
          })}
        </div>

        {/* Center 72px clear zone for JARVIS's Eyes */}
        <div className="brow-eyes-slot" aria-hidden="true" />

        <div className="brow-right">
          {ttsSpeaking ? (
            <button
              type="button"
              className="brow-live-pill is-tts"
              title="Остановить озвучку"
              onClick={() => void stopTts()}
            >
              <EqualizerBars playing accent="amber" />
              <span>Стоп</span>
            </button>
          ) : null}

          <HeadsetChip />

          <button
            type="button"
            className="icon-btn icon-btn-sm"
            aria-label="Игры с Джарвисом"
            title="Игры с Джарвисом"
            onClick={() => {
              if (isTauri()) void invoke("games_open", { game: null, difficulty: null, mood: null });
              else window.open("/qa/games.html", "_blank");
            }}
          >
            <Icon name="gamepad" size={14} />
          </button>

          <button
            type="button"
            className="icon-btn icon-btn-sm"
            aria-label="Календарь"
            title="Планировщик"
            onClick={() => isTauri() && void invoke("planner_open", { date: null })}
          >
            <Icon name="calendar" size={14} />
          </button>

          <button
            type="button"
            className="icon-btn icon-btn-sm"
            aria-label="Голосовой режим"
            title="Голосовой режим"
            onClick={() => {
              if (isTauri()) void toggleVoice();
              else {
                useVoice.setState({ state: "listening", error: null, userLine: "", assistantLine: "" });
                setIsland({ voiceOpen: true });
              }
            }}
          >
            <Icon name="mic" size={14} />
          </button>
          <button
            type="button"
            className="icon-btn icon-btn-sm"
            aria-label="Открыть чат"
            title="Рабочее пространство"
            onClick={() => setIsland({ workspaceOpen: true })}
          >
            <Icon name="chat" size={14} />
          </button>
          <button
            type="button"
            className="icon-btn icon-btn-sm"
            aria-label="Настройки"
            title="Настройки Джарвиса"
            onClick={() => void ipc.settings.open()}
          >
            <Icon name="settings" size={14} />
          </button>
          <button
            type="button"
            className={`icon-btn icon-btn-sm pin-btn${pinned ? " is-on" : ""}`}
            aria-label={pinned ? "Открепить остров" : "Закрепить остров открытым"}
            aria-pressed={pinned}
            title={pinned ? "Открепить: остров снова будет закрываться сам" : "Закрепить: остров останется открытым"}
            onClick={togglePinned}
          >
            <Icon name={pinned ? "pinned" : "pin"} size={14} />
          </button>
        </div>
      </div>

      <AgentMissionBar />
      <JobsStrip />
      <ClocksStrip />

      {/* Main Deck Stage */}
      <div className="peek-stage">
        <AnimatePresence mode="popLayout" initial={false} custom={deckDir}>
          {showFocusBanner ? (
            <motion.div
              key="focus-banner"
              className="peek-deck-pane"
              initial={{ opacity: 0, y: -3 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: 3 }}
              transition={{ duration: 0.14, ease: [0.22, 1, 0.36, 1] }}
            >
              <FocusHoverBanner onExpand={() => setDeck("focus")} />
            </motion.div>
          ) : (
            <motion.div
              key={peekDeck}
              data-deck={peekDeck}
              className="peek-deck-pane"
              custom={deckDir}
              variants={reduced ? deckFadeVariants : deckVariants}
              initial="enter"
              animate="center"
              exit="exit"
              transition={reduced ? { duration: 0.14 } : DECK_SPRING}
            >
              {peekDeck === "nook" && (
                <div className="nook-split">
                  <div className={`nook-left${withMedia ? "" : " is-now"}`}>
                    {withMedia ? (
                      <>
                        <MediaCard media={media} />
                        <NowLine compact />
                      </>
                    ) : (
                      <>
                        <NowLine />
                        <IdlePlayer />
                      </>
                    )}
                  </div>

                  <div className="nook-hairline" aria-hidden="true" />

                  <div className="nook-right">
                    <WeekCalendar />
                  </div>
                </div>
              )}

              {peekDeck === "focus" && <FocusDeck />}
              {peekDeck === "tray" && <TrayDeck />}
              {peekDeck === "growth" && <GrowthDeck />}
              {peekDeck === "activity" && <ActivityDeck />}
              {peekDeck === "sound" && <SoundDeck />}
            </motion.div>
          )}
        </AnimatePresence>
      </div>

    </div>
  );
}

/** Time, date and the weather where the user is. Without music it is the
 *  column's face (big light clock, date, weather line); with music it shrinks
 *  to one quiet mono line under the player. The weather opens the full card. */
function NowLine({ compact = false }: { compact?: boolean }) {
  const here = useWeather((s) => s.here);
  const loadHere = useWeather((s) => s.loadHere);
  const show = useWeather((s) => s.show);
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    void loadHere();
  }, [loadHere]);
  useEffect(() => {
    // Tick on the minute, not every second.
    let t = window.setTimeout(function tick() {
      setNow(new Date());
      t = window.setTimeout(tick, 60_000 - (Date.now() % 60_000) + 50);
    }, 60_000 - (Date.now() % 60_000) + 50);
    return () => window.clearTimeout(t);
  }, []);
  const time = now.toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit" });
  if (compact) {
    const short = now.toLocaleDateString("ru-RU", { weekday: "short", day: "numeric", month: "short" });
    return (
      <div className="now-strip">
        <span className="now-strip-time">{time}</span>
        <span className="now-strip-dot" aria-hidden="true" />
        <span className="now-strip-date">{short.replace(/\.$/, "")}</span>
        {here && (
          <button type="button" className="now-strip-wx" onClick={() => show(here)} title="Погода подробнее">
            <WeatherGlyph kind={here.kind} night={!here.isDay} size={12} />
            <span>{deg(here.temp)}</span>
          </button>
        )}
      </div>
    );
  }
  const weekday = now.toLocaleDateString("ru-RU", { weekday: "long" });
  const day = now.toLocaleDateString("ru-RU", { day: "numeric", month: "long" });
  return (
    <div className="now-face">
      <div className="now-face-date">
        <span className="now-face-weekday">{weekday}</span>
        <span>{day}</span>
      </div>
      <div className="now-face-time">{time}</div>
      {here && (
        <button type="button" className="now-face-wx" onClick={() => show(here)} title="Погода подробнее">
          <WeatherGlyph kind={here.kind} night={!here.isDay} size={15} />
          <span className="now-face-temp">{deg(here.temp)}</span>
          <span className="now-face-text">{here.text.toLowerCase()}</span>
          <span className="now-face-range">
            {deg(here.today.max)} / {deg(here.today.min)}
          </span>
        </button>
      )}
    </div>
  );
}
