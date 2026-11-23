// New versions of JARVIS (native `updater`): the island shows a card once per
// version, Settings → Основное always shows the state.

import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { errorMessage, isTauri, on } from "../lib/ipc";

export interface UpdateInfo {
  version: string;
  current: string;
  notes: string;
  date: string | null;
}

export interface UpdateStatus {
  current: string;
  available: UpdateInfo | null;
  checkedAt: number | null;
  installing: boolean;
}

interface UpdatesStore {
  status: UpdateStatus | null;
  /** The island card is up for `status.available`. */
  card: boolean;
  /** Bytes so far while installing. */
  progress: { downloaded: number; total: number | null } | null;
  error: string | null;
  checking: boolean;
  check: () => Promise<void>;
  install: () => Promise<void>;
  dismiss: () => void;
}

const SEEN_KEY = "jarvis.updateSeen";

function seen(version: string): boolean {
  try {
    return localStorage.getItem(SEEN_KEY) === version;
  } catch {
    return false;
  }
}

export const useUpdates = create<UpdatesStore>((set, get) => ({
  status: null,
  card: false,
  progress: null,
  error: null,
  checking: false,
  check: async () => {
    if (!isTauri()) return;
    set({ checking: true, error: null });
    try {
      await invoke("update_check");
      set({ status: await invoke<UpdateStatus>("update_status") });
    } catch (e) {
      set({ error: errorMessage(e) });
    } finally {
      set({ checking: false });
    }
  },
  install: async () => {
    if (!isTauri()) return;
    set({ error: null, progress: { downloaded: 0, total: null } });
    try {
      await invoke("update_install");
    } catch (e) {
      set({ error: errorMessage(e), progress: null });
    }
  },
  dismiss: () => {
    const v = get().status?.available?.version;
    try {
      if (v) localStorage.setItem(SEEN_KEY, v);
    } catch {
      /* the card may come back next start */
    }
    set({ card: false });
  },
}));

export async function startUpdatesFeed() {
  if (!isTauri()) return;
  await on("update:available", (available) => {
    useUpdates.setState((s) => ({
      status: { ...(s.status ?? { current: available.current, checkedAt: null, installing: false }), available },
      card: !seen(available.version),
    }));
  });
  await on("update:progress", (progress) => useUpdates.setState({ progress }));
  try {
    const status = await invoke<UpdateStatus>("update_status");
    useUpdates.setState({ status, card: Boolean(status.available && !seen(status.available.version)) });
  } catch {
    /* the native side announces a new version by event */
  }
}
