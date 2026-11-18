// Multi-deck Command Notch (`PeekView`):
// Synthesizes NotchNook (`Обзор` & `Полка`), Needt (`Фокус` hover banner +
// expanded 2-column Bento), and Reigne (`Стрик` emerald heatmap + pagination dots)
// while keeping JARVIS's centered amber `Eyes.tsx` in a strictly protected 72px
// center clear-zone.

import { AnimatePresence, motion } from "motion/react";
import { useEffect, useRef, type WheelEvent } from "react";
import { Icon, type IconName } from "../../ui/Icon";
import { ipc, isTauri } from "../../lib/ipc";
import { hasActiveMedia, useSystem } from "../../store/system";
import { useIsland } from "../../store/island";
import { usePrefs } from "../../store/prefs";
import { useHub } from "../../store/hub";
import { useTts } from "../../store/tts";
import { toggleVoice, useVoice } from "../../store/voice";
import { openClips, useClips } from "../../store/clips";
import type { PeekDeck } from "../geometry";
import { Artwork, EqualizerBars, MediaCard } from "./MediaCard";
import { QuickControls } from "./QuickControls";
import { JobsStrip } from "./JobsStrip";
import { ClocksStrip } from "./ClocksStrip";
import { WeekCalendar } from "./WeekCalendar";
import { FocusDeck, FocusHoverBanner } from "./FocusDeck";
import { TrayDeck } from "./TrayDeck";
import { ActivityDeck } from "./ActivityDeck";
import { AgentMissionBar } from "./AgentMissionBar";

const DECKS: Array<{ id: PeekDeck; label: string; icon: IconName }> = [
  { id: "nook", label: "Обзор", icon: "nook" },
  { id: "focus", label: "Фокус", icon: "target" },
  { id: "tray", label: "Полка", icon: "tray" },
  { id: "activity", label: "Стрик", icon: "grid" },
];

export function PeekView() {
  const media = useSystem((s) => s.media);
  const withMedia = hasActiveMedia(media);
  const peekDeck = useIsland((s) => s.peekDeck);
  const focusBannerDismissed = useIsland((s) => s.focusBannerDismissed);
  const forceFocusBanner = useIsland((s) => s.forceFocusBanner);
  const setIsland = useIsland((s) => s.set);
  const setDeck = useIsland((s) => s.setDeck);
  const shortcut = usePrefs((p) => p.workspaceShortcut);

  const focus = useHub((s) => s.focus);
  const shelfCount = useHub((s) => s.shelf.length);
  const fullControl = useHub((s) => s.fullControl);
  const toggleFullControl = useHub((s) => s.toggleFullControl);

  const ttsSpeaking = useTts((s) => s.speaking);
  const stopTts = useTts((s) => s.stop);

  const showFocusBanner = forceFocusBanner || (focus.active && !focusBannerDismissed);

  // Alt+1..4 switches decks rapidly.
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

  return (
    <div className="view view-peek" onWheel={onWheel}>
      {/* Top Brow: 3-column grid reserving an untouchable 72px center slot for Eyes.tsx */}
      <div className="brow brow-split">
        <div className="brow-left" role="tablist" aria-label="Разделы острова">
          {DECKS.map((d) => {
            const active = !showFocusBanner && peekDeck === d.id;
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
          ) : withMedia && (showFocusBanner || peekDeck !== "nook") ? (
            <button
              type="button"
              className="brow-live-pill is-audio"
              title={`${media.title} — открыть плеер`}
              onClick={() => setDeck("nook")}
            >
              <Artwork size={14} radius={4} />
              <span className="brow-audio-title">{media.title}</span>
              <EqualizerBars playing={media.status === "playing"} />
            </button>
          ) : null}

          <button
            type="button"
            className={`icon-btn icon-btn-sm autopilot-btn${fullControl ? " is-armed" : ""}`}
            aria-pressed={fullControl}
            aria-label="Полное управление (Автопилот)"
            title={
              fullControl
                ? "Автопилот включён: Джарвис действует самостоятельно"
                : "Включить Полное управление (Автопилот)"
            }
            onClick={() => void toggleFullControl()}
          >
            <Icon name="bolt" size={14} />
          </button>
          <button
            type="button"
            className="icon-btn icon-btn-sm"
            aria-label="История буфера обмена"
            title="Буфер обмена"
            onClick={() => {
              if (isTauri()) openClips(false);
              else {
                useClips.setState({ loaded: true, error: null });
                setIsland({ clipsOpen: true, clipsKeyboard: false, workspaceOpen: false });
              }
            }}
          >
            <Icon name="clipboard" size={14} />
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
        </div>
      </div>

      <AgentMissionBar />
      <JobsStrip />
      <ClocksStrip />

      {/* Main Deck Stage */}
      <div className="peek-stage">
        <AnimatePresence mode="wait" initial={false}>
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
              className="peek-deck-pane"
              initial={{ opacity: 0, y: 3 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -3 }}
              transition={{ duration: 0.14, ease: [0.22, 1, 0.36, 1] }}
            >
              {peekDeck === "nook" && (
                <div className="nook-split">
                  <div className="nook-left">
                    {withMedia ? (
                      <>
                        <MediaCard media={media} />
                        <div className="nook-sliders-compact">
                          <QuickControls />
                        </div>
                      </>
                    ) : (
                      <div className="nook-idle-left">
                        <button
                          type="button"
                          className="ask-pill"
                          onClick={() => setIsland({ workspaceOpen: true })}
                        >
                          <span>Спросить Джарвиса…</span>
                          {shortcut && <kbd>{shortcut.replaceAll("+", " ")}</kbd>}
                        </button>

                        <div className="nook-quick-pills">
                          <button
                            type="button"
                            className="nook-chip-btn"
                            onClick={() => setDeck("focus")}
                          >
                            <Icon name="target" size={13} />
                            <span>Фокус 25м</span>
                          </button>
                          <button
                            type="button"
                            className="nook-chip-btn"
                            onClick={() => setDeck("tray")}
                          >
                            <Icon name="tray" size={13} />
                            <span>Полка{shelfCount > 0 ? ` (${shelfCount})` : ""}</span>
                          </button>
                        </div>

                        <QuickControls />
                      </div>
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
              {peekDeck === "activity" && <ActivityDeck />}
            </motion.div>
          )}
        </AnimatePresence>
      </div>

      {/* Reference 1 minimal bottom pagination dots */}
      <div className="peek-dots" role="tablist" aria-label="Переключение панелей">
        {DECKS.map((d) => {
          const active = showFocusBanner ? d.id === "focus" : peekDeck === d.id;
          return (
            <button
              key={d.id}
              type="button"
              role="tab"
              aria-selected={active}
              aria-label={d.label}
              title={d.label}
              className={`peek-dot${active ? " is-active" : ""}`}
              onClick={() => setDeck(d.id)}
            />
          );
        })}
      </div>
    </div>
  );
}
