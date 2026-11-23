// UI-relevant preferences mirrored from native settings.

import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { hasTauri, workspaceIpc } from "../lib/workspace-ipc";

export interface Prefs {
  reduceMotion: boolean;
  workspaceShortcut: string;
  fullControl: boolean;
  autoSpeakReplies: boolean;
  /** Minutes before a calendar event the island shows it; 0 = off. */
  islandRemind: number;
  setFullControl: (enabled: boolean) => Promise<boolean>;
  setAutoSpeakReplies: (enabled: boolean) => Promise<boolean>;
}

export const usePrefs = create<Prefs>((set) => ({
  reduceMotion: false,
  workspaceShortcut: "Ctrl+Alt+J",
  fullControl: false,
  autoSpeakReplies: false,
  islandRemind: 10,

  setFullControl: async (enabled: boolean) => {
    set({ fullControl: enabled });
    const result = await workspaceIpc.agent.setFullControl(enabled);
    set({ fullControl: result });
    return result;
  },

  setAutoSpeakReplies: async (enabled: boolean) => {
    set({ autoSpeakReplies: enabled });
    if (hasTauri()) {
      try {
        const v = await invoke<{ settings: Record<string, unknown> }>("settings_get");
        if (v && v.settings) {
          const voice = (v.settings.voice as Record<string, unknown> | undefined) ?? {};
          const next = {
            ...v.settings,
            voice: {
              ...voice,
              autoSpeakReplies: enabled,
            },
          };
          await invoke("settings_save", { settings: next });
        }
      } catch {
        /* local state stays updated */
      }
    }
    return enabled;
  },
}));

async function load() {
  if (!hasTauri()) return;
  try {
    const v = await invoke<{
      settings: {
        reduceMotion?: boolean;
        shortcuts?: { workspace?: string };
        agent?: { fullControl?: boolean };
        voice?: { autoSpeakReplies?: boolean };
        planner?: { islandRemind?: number };
      };
    }>("settings_get");
    if (v?.settings) {
      usePrefs.setState({
        reduceMotion: !!v.settings.reduceMotion,
        workspaceShortcut: v.settings.shortcuts?.workspace ?? "Ctrl+Alt+J",
        fullControl: !!v.settings.agent?.fullControl,
        autoSpeakReplies: !!v.settings.voice?.autoSpeakReplies,
        islandRemind: v.settings.planner?.islandRemind ?? 10,
      });
    }
  } catch {
    /* defaults stay */
  }
}

export async function startPrefsFeed() {
  await load();
  if (!hasTauri()) return;
  try {
    await listen("settings:changed", () => void load());
  } catch {
    /* ignore */
  }
}
