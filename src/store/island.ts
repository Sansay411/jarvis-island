// Island interaction state. The visible mode is derived from a few facts
// (pointer, drag, voice, workspace) so every transition has one owner.

import { create } from "zustand";
import { ipc } from "../lib/ipc";
import type { IslandMode, PeekDeck } from "../island/geometry";

export interface DroppedFile {
  path: string;
  name: string;
}

interface IslandStore {
  hovered: boolean;
  dragOver: boolean;
  workspaceOpen: boolean;
  voiceOpen: boolean;
  /** The clipboard history panel. */
  clipsOpen: boolean;
  /** The keyboard belongs to the panel (hotkey, or a click into the search). */
  clipsKeyboard: boolean;
  visible: boolean;
  topCovered: boolean;
  /** Active deck inside PeekView (`Обзор` · `Фокус` · `Полка` · `Стрик`). */
  peekDeck: PeekDeck;
  /** When Focus is running, hovering first shows the Needt Hover Banner; clicking expands into the full deck. */
  focusBannerDismissed: boolean;
  /** Optional QA override to force showing the Needt Hover Focus Banner. */
  forceFocusBanner: boolean;
  /** True while an inline `<input>` inside the Island holds native keyboard focus. */
  inputFocused: boolean;
  pendingAttachments: DroppedFile[];
  set: (patch: Partial<Omit<IslandStore, "set" | "addAttachments" | "setDeck">>) => void;
  setDeck: (deck: PeekDeck) => void;
  addAttachments: (files: DroppedFile[]) => void;
}

export const useIsland = create<IslandStore>((set) => ({
  hovered: false,
  dragOver: false,
  workspaceOpen: false,
  voiceOpen: false,
  clipsOpen: false,
  clipsKeyboard: false,
  visible: true,
  topCovered: false,
  peekDeck: "nook",
  focusBannerDismissed: false,
  forceFocusBanner: false,
  inputFocused: false,
  pendingAttachments: [],
  set: (patch) => set(patch),
  setDeck: (peekDeck) => set({ peekDeck, focusBannerDismissed: true, forceFocusBanner: false }),
  addAttachments: (files) =>
    set((s) => {
      const known = new Set(s.pendingAttachments.map((f) => f.path));
      return { pendingAttachments: [...s.pendingAttachments, ...files.filter((f) => !known.has(f.path))] };
    }),
}));

let inputFocusCount = 0;
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
  s: Pick<IslandStore, "hovered" | "dragOver" | "workspaceOpen" | "voiceOpen" | "clipsOpen" | "inputFocused">,
): IslandMode {
  // Only a real file drag opens the drop zone: the native drop catcher
  // reports «enter» once Windows hands it files, so a plain press-and-drag
  // near the island changes nothing.
  if (s.dragOver) return "drop";
  // A live voice session takes over; the workspace returns when it ends.
  if (s.voiceOpen) return "voice";
  if (s.workspaceOpen) return "workspace";
  if (s.clipsOpen) return "clips";
  if (s.hovered || s.inputFocused) return "peek";
  return "compact";
}
