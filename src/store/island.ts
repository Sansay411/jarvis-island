// Island interaction state. The visible mode is derived from a few facts
// (pointer, drag, voice, workspace) so every transition has one owner.

import { create } from "zustand";
import { ipc } from "../lib/ipc";
import type { IslandMode, PeekDeck } from "../island/geometry";
import { deckDirection } from "../island/motion";

export interface DroppedFile {
  path: string;
  name: string;
}

interface IslandStore {
  hovered: boolean;
  dragOver: boolean;
  /** A file drag is over the «В Telegram» zone of the drop view. */
  dropOnSend: boolean;
  workspaceOpen: boolean;
  voiceOpen: boolean;
  /** Voice listening was auto-started right after picking a screen area: keep
   *  the workspace visible so the user can either type or speak immediately. */
  areaVoiceInvited: boolean;
  /** The clipboard history panel. */
  clipsOpen: boolean;
  /** The keyboard belongs to the panel (hotkey, or a click into the search). */
  clipsKeyboard: boolean;
  visible: boolean;
  topCovered: boolean;
  /** Active deck inside PeekView (`Обзор` · `Фокус` · `Полка` · `Стрик`). */
  peekDeck: PeekDeck;
  /** Side the last deck change came from (+1 right, -1 left) for the slide. */
  deckDir: 1 | -1 | 0;
  /** «Рост» shows a card to answer right on the island (a taller deck). */
  growthCard: boolean;
  /** When Focus is running, hovering first shows the Needt Hover Banner; clicking expands into the full deck. */
  focusBannerDismissed: boolean;
  /** Optional QA override to force showing the Needt Hover Focus Banner. */
  forceFocusBanner: boolean;
  /** True while an inline `<input>` inside the Island holds native keyboard focus. */
  inputFocused: boolean;
  /** The user pinned the island open: it stays expanded when the pointer
   *  leaves or the user clicks elsewhere, until unpinned. Kept across restarts. */
  pinned: boolean;
  pendingAttachments: DroppedFile[];
  set: (patch: Partial<Omit<IslandStore, "set" | "addAttachments" | "setDeck" | "togglePinned">>) => void;
  setDeck: (deck: PeekDeck) => void;
  togglePinned: () => void;
  addAttachments: (files: DroppedFile[]) => void;
}

const PINNED_KEY = "jarvis.islandPinned";

function readPinned(): boolean {
  try {
    return typeof localStorage !== "undefined" && localStorage.getItem(PINNED_KEY) === "1";
  } catch {
    return false;
  }
}

export const useIsland = create<IslandStore>((set) => ({
  hovered: false,
  dragOver: false,
  dropOnSend: false,
  workspaceOpen: false,
  voiceOpen: false,
  areaVoiceInvited: false,
  clipsOpen: false,
  clipsKeyboard: false,
  visible: true,
  topCovered: false,
  peekDeck: "nook",
  deckDir: 0,
  growthCard: false,
  focusBannerDismissed: false,
  forceFocusBanner: false,
  inputFocused: false,
  pinned: readPinned(),
  pendingAttachments: [],
  set: (patch) => set(patch),
  togglePinned: () =>
    set((s) => {
      const pinned = !s.pinned;
      try {
        localStorage.setItem(PINNED_KEY, pinned ? "1" : "0");
      } catch {
        /* remembered for this session only */
      }
      // Unpinning with the pointer away closes the island right away.
      return pinned ? { pinned } : { pinned, hovered: false, focusBannerDismissed: false };
    }),
  setDeck: (peekDeck) =>
    set((s) =>
      s.peekDeck === peekDeck
        ? { focusBannerDismissed: true, forceFocusBanner: false }
        : { peekDeck, deckDir: deckDirection(s.peekDeck, peekDeck), focusBannerDismissed: true, forceFocusBanner: false },
    ),
  addAttachments: (files) =>
    set((s) => {
      const known = new Set(s.pendingAttachments.map((f) => f.path));
      return { pendingAttachments: [...s.pendingAttachments, ...files.filter((f) => !known.has(f.path))] };
    }),
}));

let inputFocusCount = 0;
export function endInlineInput(blur: () => void): Promise<void> {
  blur();
  window.clearTimeout(blurReleaseTimer);
  inputFocusCount = 0;
  useIsland.setState({ inputFocused: false, hovered: false, focusBannerDismissed: false });
  return ipc.island.endInput(true);
}
let blurReleaseTimer: number | undefined;

/** Wire onto inline `<input>` elements inside PeekView so Windows grants
 *  keyboard focus to the non-activating overlay and restores it on blur. */
export function islandInputHandlers() {
  return {
    onFocus: () => {
      window.clearTimeout(blurReleaseTimer);
      inputFocusCount += 1;
      useIsland.setState({ inputFocused: true });
      void ipc.island.beginInput();
    },
    onBlur: () => {
      inputFocusCount = Math.max(0, inputFocusCount - 1);
      window.clearTimeout(blurReleaseTimer);
      blurReleaseTimer = window.setTimeout(() => {
        if (inputFocusCount === 0) {
          useIsland.setState({ inputFocused: false });
          const st = useIsland.getState();
          if (!st.workspaceOpen && !(st.clipsOpen && st.clipsKeyboard)) {
            void ipc.island.endInput(true);
          }
        }
      }, 90);
    },
  };
}

export function resolveMode(
  s: Pick<IslandStore, "hovered" | "dragOver" | "workspaceOpen" | "voiceOpen" | "clipsOpen" | "inputFocused"> & {
    pinned?: boolean;
    areaVoiceInvited?: boolean;
  },
): IslandMode {
  // Only a real file drag opens the drop zone: the native drop catcher
  // reports «enter» once Windows hands it files, so a plain press-and-drag
  // near the island changes nothing.
  if (s.dragOver) return "drop";
  // A live voice session takes over, except while the workspace is open with
  // an invited mic right after selecting a screen area (user can type or speak).
  if (s.voiceOpen && !(s.workspaceOpen && s.areaVoiceInvited)) return "voice";
  if (s.workspaceOpen) return "workspace";
  if (s.clipsOpen) return "clips";
  // Pinned: the opened island stays, whatever the pointer does.
  if (s.hovered || s.inputFocused || s.pinned) return "peek";
  return "compact";
}
