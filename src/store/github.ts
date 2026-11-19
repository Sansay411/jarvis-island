// The user's GitHub contribution calendar for the «Стрик» deck. The native
// side fetches and caches it (`github.rs`); this store only holds what the
// deck shows and whether a request is in flight.

import { create } from "zustand";
import { errorMessage, ipc, isTauri } from "../lib/ipc";
import type { GithubState } from "../lib/types";

interface GithubStore {
  state: GithubState | null;
  loading: boolean;
  /** A failed save of the login (shown under the field). */
  formError: string | null;
  load: (force?: boolean) => Promise<void>;
  connect: (login: string) => Promise<void>;
}

export const useGithub = create<GithubStore>((set, get) => ({
  state: null,
  loading: false,
  formError: null,
  load: async (force = false) => {
    if (!isTauri() || get().loading) return;
    set({ loading: true });
    try {
      set({ state: await ipc.hub.githubGet(force) });
    } catch (e) {
      set((s) => ({ state: { login: s.state?.login ?? null, activity: s.state?.activity ?? null, error: errorMessage(e) } }));
    } finally {
      set({ loading: false });
    }
  },
  connect: async (login) => {
    if (!isTauri()) return;
    set({ loading: true, formError: null });
    try {
      const st = await ipc.hub.githubSetLogin(login);
      set({ state: st, formError: st.activity ? null : st.error });
    } catch (e) {
      set({ formError: errorMessage(e) });
    } finally {
      set({ loading: false });
    }
  },
}));
