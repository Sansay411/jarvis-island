// Clipboard history on the island. The list, search, pinning and pasting all
// live on the native side (`clips_*`); this store holds what the panel shows.

import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { errorMessage } from "../lib/ipc";
import { useIsland } from "./island";
import { emote } from "./emote";

export type ClipKind = "text" | "image" | "files";

export interface Clip {
  id: number;
  kind: ClipKind;
  /** One line of the text, the file names, or «Изображение 1920×1080». */
  preview: string;
  size: number;
  width: number | null;
  height: number | null;
  /** Small PNG data URL for pictures. */
  thumb: string | null;
  /** The program that copied it. */
  source: string;
  pinned: boolean;
  createdAt: string;
  usedAt: string;
}

interface ClipsStore {
  items: Clip[];
  query: string;
  /** Index of the row Enter pastes. */
  selected: number;
  loaded: boolean;
  /** History is recording (the panel's «Пауза» turns it off). */
  enabled: boolean;
  shortcut: string;
  error: string | null;
  busy: boolean;
  /** Measured height of the panel (px). */
  height: number;
}

export const useClips = create<ClipsStore>(() => ({
  items: [],
  query: "",
  selected: 0,
  loaded: false,
  enabled: true,
  shortcut: "Ctrl+Alt+V",
  error: null,
  busy: false,
  height: 0,
}));

const LIMIT = 60;
// A slow reply must not overwrite a newer one.
let seq = 0;
let typing: number | undefined;

export async function loadClips() {
  const my = ++seq;
  try {
    const items = await invoke<Clip[]>("clips_list", { query: useClips.getState().query, limit: LIMIT });
    if (my !== seq) return;
    useClips.setState((s) => ({ items, loaded: true, error: null, selected: Math.min(s.selected, Math.max(0, items.length - 1)) }));
  } catch (e) {
    if (my === seq) useClips.setState({ loaded: true, error: errorMessage(e) });
  }
}

async function refreshStatus() {
  try {
    const s = await invoke<{ enabled: boolean; shortcut: string }>("clips_status");
    useClips.setState({ enabled: s.enabled, shortcut: s.shortcut });
  } catch {
    /* defaults stay */
  }
}

export function setQuery(query: string) {
  useClips.setState({ query, selected: 0 });
  window.clearTimeout(typing);
  typing = window.setTimeout(() => void loadClips(), 110);
}

export function moveSelection(delta: number) {
  useClips.setState((s) => ({ selected: s.items.length === 0 ? 0 : (s.selected + delta + s.items.length) % s.items.length }));
}

export function selectRow(index: number) {
  if (useClips.getState().selected !== index) useClips.setState({ selected: index });
}

/** `keyboard`: opened with the hotkey, so the search takes the keyboard. */
export function openClips(keyboard: boolean) {
  useClips.setState({ query: "", selected: 0, error: null });
  // The panel replaces the chat; the two never share the island.
  useIsland.getState().set({ clipsOpen: true, clipsKeyboard: keyboard, workspaceOpen: false });
  void loadClips();
  void refreshStatus();
}

export function closeClips() {
  window.clearTimeout(typing);
  useIsland.getState().set({ clipsOpen: false, clipsKeyboard: false });
}

export function toggleClips() {
  if (useIsland.getState().clipsOpen) closeClips();
  else openClips(true);
}

/** Put the entry on the clipboard and paste it where the user was typing. */
export async function pasteClip(id: number) {
  if (useClips.getState().busy) return;
  useClips.setState({ busy: true, error: null });
  try {
    await invoke<string>("clips_paste", { id });
    closeClips();
    emote("wink");
  } catch (e) {
    useClips.setState({ error: errorMessage(e) });
  } finally {
    useClips.setState({ busy: false });
  }
}

export async function pinClip(id: number, pinned: boolean) {
  try {
    await invoke("clips_pin", { id, pinned });
    await loadClips();
  } catch (e) {
    useClips.setState({ error: errorMessage(e) });
  }
}

export async function removeClip(id: number) {
  try {
    await invoke("clips_delete", { id });
    await loadClips();
  } catch (e) {
    useClips.setState({ error: errorMessage(e) });
  }
}

/** Everything but the pinned entries. */
export async function clearClips() {
  try {
    await invoke<number>("clips_clear", { keepPinned: true });
    await loadClips();
  } catch (e) {
    useClips.setState({ error: errorMessage(e) });
  }
}

export async function setRecording(enabled: boolean) {
  try {
    await invoke("clips_set_enabled", { enabled });
    useClips.setState({ enabled });
  } catch (e) {
    useClips.setState({ error: errorMessage(e) });
  }
}

/** «5 мин назад», «вчера»… */
export function ago(iso: string, now = Date.now()): string {
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return "";
  const min = Math.floor((now - t) / 60_000);
  if (min < 1) return "только что";
  if (min < 60) return `${min} мин назад`;
  const h = Math.floor(min / 60);
  if (h < 24) return `${h} ч назад`;
  const d = Math.floor(h / 24);
  return d === 1 ? "вчера" : `${d} дн. назад`;
}

export async function startClipsFeed() {
  await listen("clips:changed", () => {
    if (useIsland.getState().clipsOpen) void loadClips();
  });
  await listen("settings:changed", () => void refreshStatus());
  await refreshStatus();
}
