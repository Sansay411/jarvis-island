// Typed wrappers over Tauri commands and events. The rest of the UI never
// calls `invoke`/`listen` with raw strings, and standalone Vite browser
// previews degrade cleanly without throwing when Tauri internals are absent.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  ActivityHeatmap,
  GithubState,
  AppErrorPayload,
  ArtworkEvent,
  AudioState,
  DisplayBrightness,
  FocusState,
  IslandCommand,
  MediaAction,
  MediaControlOutcome,
  MediaState,
  PointerEvent,
  PresenceEvent,
  RoutineItem,
  RoutineSaveInput,
  ShapeRect,
  ShelfItem,
  TaskColor,
  TaskItem,
  TtsBenchResult,
  TtsSpeakOptions,
  TtsSpeakOutcome,
  TtsStatePayload,
} from "./types";

export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export function isAppError(e: unknown): e is AppErrorPayload {
  return typeof e === "object" && e !== null && "code" in e && "message" in e;
}

export function errorMessage(e: unknown): string {
  if (isAppError(e)) return e.message;
  if (e instanceof Error) return e.message;
  return String(e);
}

const noopAsync = async (): Promise<void> => {};

export const ipc = {
  island: {
    setShape: (shape: ShapeRect) => (isTauri() ? invoke<void>("island_set_shape", { shape }) : noopAsync()),
    beginInput: () => (isTauri() ? invoke<void>("island_begin_input") : noopAsync()),
    endInput: (restore: boolean) => (isTauri() ? invoke<void>("island_end_input", { restore }) : noopAsync()),
    setHidden: (hidden: boolean) => (isTauri() ? invoke<void>("island_set_hidden", { hidden }) : noopAsync()),
  },
  settings: {
    open: () => (isTauri() ? invoke<void>("settings_open") : noopAsync()),
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
    /** The playing app's own icon as Windows draws it (PNG data URL). */
    appIcon: (aumid: string) =>
      isTauri() ? invoke<string | null>("media_app_icon", { aumid }) : Promise.resolve<string | null>(null),
    control: (action: MediaAction) => invoke<MediaControlOutcome>("media_control", { action }),
    seek: (positionMs: number) =>
      isTauri() ? invoke<void>("media_seek", { positionMs: Math.max(0, Math.round(positionMs)) }) : noopAsync(),
  },
  tts: {
    speak: (text: string, opts?: TtsSpeakOptions) =>
      invoke<TtsSpeakOutcome>("tts_speak", {
        text,
        voice: opts?.voice ?? null,
        rate: opts?.rate ?? null,
        interrupt: opts?.interrupt ?? true,
      }),
    stop: () => invoke<void>("tts_stop"),
    status: () => invoke<TtsStatePayload>("tts_status"),
    benchmark: (voice?: string | null) => invoke<TtsBenchResult>("tts_benchmark", { voice: voice ?? null }),
  },
  weather: {
    get: (place?: string) => invoke<import("../store/weather").Weather>("weather_get", { place: place ?? null }),
  },
  hub: {
    tasksList: () => invoke<TaskItem[]>("hub_tasks_list"),
    taskAdd: (
      title: string,
      opts?: { scheduledDate?: string | null; scheduledTime?: string | null; color?: TaskColor | string | null },
    ) =>
      invoke<TaskItem>("hub_task_add", {
        title,
        scheduledDate: opts?.scheduledDate ?? null,
        scheduledTime: opts?.scheduledTime ?? null,
        color: opts?.color ?? null,
      }),
    taskToggle: (id: string) => invoke<TaskItem>("hub_task_toggle", { id }),
    taskRemove: (id: string) => invoke<void>("hub_task_remove", { id }),

    focusGet: () => invoke<FocusState>("hub_focus_get"),
    focusStart: (title: string, minutes: number, taskId?: string | null) =>
      invoke<FocusState>("hub_focus_start", {
        title,
        minutes: Math.max(1, Math.round(minutes)),
        taskId: taskId ?? null,
      }),
    focusPauseToggle: () => invoke<FocusState>("hub_focus_pause_toggle"),
    focusComplete: () => invoke<FocusState>("hub_focus_complete"),
    focusCancel: () => invoke<FocusState>("hub_focus_cancel"),

    shelfList: () => invoke<ShelfItem[]>("hub_shelf_list"),
    shelfAddFiles: (paths: string[]) => invoke<ShelfItem[]>("hub_shelf_add_files", { paths }),
    shelfAddText: (text: string) => invoke<ShelfItem>("hub_shelf_add_text", { text }),
    shelfRemove: (id: string) => invoke<void>("hub_shelf_remove", { id }),
    shelfClear: () => invoke<void>("hub_shelf_clear"),
    shelfThumbnail: (path: string) => invoke<string | null>("hub_shelf_thumbnail", { path }),
    shelfDragOut: (paths: string[]) => invoke<boolean>("hub_shelf_drag_out", { paths }),

    activityGet: () => invoke<ActivityHeatmap>("hub_activity_get"),
    githubGet: (force = false) => invoke<GithubState>("hub_github_get", { force }),
    githubSetLogin: (login: string) => invoke<GithubState>("hub_github_set_login", { login }),

    routinesList: () => invoke<RoutineItem[]>("hub_routines_list"),
    routineSave: (input: RoutineSaveInput) =>
      invoke<RoutineItem>("hub_routine_save", {
        id: input.id ?? null,
        title: input.title,
        subtitle: input.subtitle,
        icon: input.icon,
        prompt: input.prompt,
        autonomy: input.autonomy,
      }),
    routineRemove: (id: string) => invoke<void>("hub_routine_remove", { id }),
    routineRun: (id: string) => invoke<{ turnId: string; conversationId: string }>("hub_routine_run", { id }),

    setFullControl: (enabled: boolean) => invoke<boolean>("agent_set_full_control", { enabled }),
  },
};

type EventMap = {
  "island:pointer": PointerEvent;
  "island:presence": PresenceEvent;
  "island:file-drop": { type: "enter" | "over" | "leave" | "drop"; paths: string[]; x?: number | null; y?: number | null };
  "island:context": { topCovered: boolean };
  "island:blur": null;
  "island:command": IslandCommand;
  "media:changed": MediaState;
  "media:artwork": ArtworkEvent;
  "tts:state": TtsStatePayload;
  "hub:tasks": TaskItem[];
  "hub:focus": FocusState;
  "hub:shelf": ShelfItem[];
  "hub:routines": RoutineItem[];
  "settings:changed": unknown;
};

export function on<K extends keyof EventMap>(event: K, handler: (payload: EventMap[K]) => void): Promise<UnlistenFn> {
  if (!isTauri()) return Promise.resolve(() => {});
  return listen<EventMap[K]>(event, (e) => handler(e.payload));
}
