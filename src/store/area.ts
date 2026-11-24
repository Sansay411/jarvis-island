// Pending screen-area attachment mirrored from the native AreaManager.
// Shared by WorkspaceView (thumbnail chip above the composer) and VoiceView
// («Область прикреплена» chip when circling while talking).

import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { isTauri } from "../lib/ipc";
import type { PendingAreaPreview } from "../lib/agent-types";

export interface CircledAreaEvent {
  rect: { x: number; y: number; width: number; height: number };
  width: number;
  height: number;
}

interface AreaStore {
  pending: PendingAreaPreview | null;
  lastCircled: CircledAreaEvent | null;
  setPending: (area: PendingAreaPreview | null) => void;
  clear: () => Promise<void>;
  openPicker: () => Promise<void>;
}

export const useArea = create<AreaStore>((set) => ({
  pending: null,
  lastCircled: null,
  setPending: (pending) => set({ pending }),
  clear: async () => {
    set({ pending: null });
    if (isTauri()) {
      try {
        await invoke("area_clear");
      } catch {
        /* ignore */
      }
    }
  },
  openPicker: async () => {
    if (!isTauri()) return;
    await invoke("area_open_picker");
  },
}));

export async function startAreaFeed() {
  if (!isTauri()) return;
  try {
    const current = await invoke<PendingAreaPreview | null>("area_pending");
    if (current) useArea.setState({ pending: current });
  } catch {
    /* ignore */
  }
  await listen<PendingAreaPreview | null>("area:pending", (e) => {
    useArea.setState({ pending: e.payload ?? null });
  });
  await listen<CircledAreaEvent>("area:circled", (e) => {
    useArea.setState({ lastCircled: e.payload });
  });
}
