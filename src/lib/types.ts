// Shapes mirrored from the Rust IPC layer (serde camelCase).

export interface AppErrorPayload {
  code:
    | "platform"
    | "unsupported"
    | "invalid_input"
    | "not_found"
    | "permission_denied"
    | "network"
    | "provider"
    | "cancelled"
    | "timeout"
    | "storage"
    | "internal";
  message: string;
}

export interface ShapeRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface PointerEvent {
  inside: boolean;
  dragApproach: boolean;
}

export interface PresenceEvent {
  visible: boolean;
  reason: "fullscreen" | "user";
}

export type IslandCommand = "toggle-workspace" | "open-workspace" | "toggle-voice" | "cancel";

export interface AudioState {
  volume: number;
  muted: boolean;
  device: string;
}

export interface DisplayBrightness {
  id: string;
  name: string;
  kind: "internal" | "external";
  brightness: number | null;
  supported: boolean;
}

export type PlaybackStatus = "playing" | "paused" | "stopped" | "changing" | "opened" | "closed" | "";

export interface MediaState {
  available: boolean;
  sourceApp: string;
  title: string;
  artist: string;
  album: string;
  status: PlaybackStatus;
  positionMs: number | null;
  durationMs: number | null;
  positionSampledAt: number | null;
  canToggle: boolean;
  canNext: boolean;
  canPrevious: boolean;
  trackKey: string;
}

export interface ArtworkEvent {
  trackKey: string;
  dataUrl: string | null;
}

export type MediaAction = "play_pause" | "play" | "pause" | "next" | "previous" | "stop";

export interface MediaControlOutcome {
  state: MediaState;
  method: "session" | "media_key";
  verified: boolean;
}

export interface WindowInfo {
  hwnd: number;
  title: string;
  process: string;
  pid: number;
  className: string;
  state: "normal" | "minimized" | "maximized";
  foreground: boolean;
  rect: { x: number; y: number; width: number; height: number };
  monitor: string;
}

export interface AppEntry {
  name: string;
  id: string;
  exe: string | null;
  kind: "packaged" | "desktop";
}
