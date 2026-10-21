// Typed wrappers over Tauri commands and events. The rest of the UI never
// calls `invoke`/`listen` with raw strings.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AppErrorPayload,
  ArtworkEvent,
  AudioState,
  DisplayBrightness,
  IslandCommand,
  MediaAction,
  MediaControlOutcome,
  MediaState,
  PointerEvent,
  PresenceEvent,
  ShapeRect,
} from "./types";

export function isAppError(e: unknown): e is AppErrorPayload {
  return typeof e === "object" && e !== null && "code" in e && "message" in e;
}

export function errorMessage(e: unknown): string {
  if (isAppError(e)) return e.message;
  if (e instanceof Error) return e.message;
  return String(e);
}

export const ipc = {
  island: {
    setShape: (shape: ShapeRect) => invoke<void>("island_set_shape", { shape }),
    beginInput: () => invoke<void>("island_begin_input"),
    endInput: (restore: boolean) => invoke<void>("island_end_input", { restore }),
    setHidden: (hidden: boolean) => invoke<void>("island_set_hidden", { hidden }),
  },
  audio: {
    get: () => invoke<AudioState>("audio_get"),
    setVolume: (percent: number) => invoke<AudioState>("audio_set_volume", { percent: Math.round(percent) }),
    setMute: (muted: boolean) => invoke<AudioState>("audio_set_mute", { muted }),
  },
  brightness: {
    list: () => invoke<DisplayBrightness[]>("brightness_list"),
    set: (percent: number, target?: string) =>
      invoke<DisplayBrightness[]>("brightness_set", { percent: Math.round(percent), target: target ?? null }),
  },
  media: {
    get: () => invoke<MediaState>("media_get"),
    artwork: () => invoke<ArtworkEvent>("media_artwork"),
    control: (action: MediaAction) => invoke<MediaControlOutcome>("media_control", { action }),
  },
};

type EventMap = {
  "island:pointer": PointerEvent;
  "island:presence": PresenceEvent;
  "island:blur": null;
  "island:command": IslandCommand;
  "media:changed": MediaState;
  "media:artwork": ArtworkEvent;
};

export function on<K extends keyof EventMap>(event: K, handler: (payload: EventMap[K]) => void): Promise<UnlistenFn> {
  return listen<EventMap[K]>(event, (e) => handler(e.payload));
}
