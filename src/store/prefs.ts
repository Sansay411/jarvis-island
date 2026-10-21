// UI-relevant preferences mirrored from native settings.

import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

interface Prefs {
  reduceMotion: boolean;
  workspaceShortcut: string;
}

export const usePrefs = create<Prefs>(() => ({ reduceMotion: false, workspaceShortcut: "Ctrl+Alt+J" }));

async function load() {
  try {
    const v = await invoke<{ settings: { reduceMotion: boolean; shortcuts: { workspace: string } } }>("settings_get");
    usePrefs.setState({ reduceMotion: v.settings.reduceMotion, workspaceShortcut: v.settings.shortcuts.workspace });
  } catch {
    /* defaults stay */
  }
}

export async function startPrefsFeed() {
  await load();
  await listen("settings:changed", () => void load());
}
