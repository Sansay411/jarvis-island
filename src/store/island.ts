// Island interaction state. The visible mode is derived from a few facts
// (pointer, drag, voice, workspace) so every transition has one owner.

import { create } from "zustand";
import type { IslandMode } from "../island/geometry";

export interface DroppedFile {
  path: string;
  name: string;
}

interface IslandStore {
  hovered: boolean;
  dragOver: boolean;
  workspaceOpen: boolean;
  voiceOpen: boolean;
  visible: boolean;
  topCovered: boolean;
  pendingAttachments: DroppedFile[];
  set: (patch: Partial<Omit<IslandStore, "set" | "addAttachments">>) => void;
  addAttachments: (files: DroppedFile[]) => void;
}

export const useIsland = create<IslandStore>((set) => ({
  hovered: false,
  dragOver: false,
  workspaceOpen: false,
  voiceOpen: false,
  visible: true,
  topCovered: false,
  pendingAttachments: [],
  set: (patch) => set(patch),
  addAttachments: (files) =>
    set((s) => {
      const known = new Set(s.pendingAttachments.map((f) => f.path));
      return { pendingAttachments: [...s.pendingAttachments, ...files.filter((f) => !known.has(f.path))] };
    }),
}));

export function resolveMode(s: Pick<IslandStore, "hovered" | "dragOver" | "workspaceOpen" | "voiceOpen">): IslandMode {
  // Only a real file drag opens the drop zone: the native drop catcher
  // reports «enter» once Windows hands it files, so a plain press-and-drag
  // near the island changes nothing.
  if (s.dragOver) return "drop";
  // A live voice session takes over; the workspace returns when it ends.
  if (s.voiceOpen) return "voice";
  if (s.workspaceOpen) return "workspace";
  if (s.hovered) return "peek";
  return "compact";
}
