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

export type IslandCommand = "toggle-workspace" | "open-workspace" | "toggle-voice" | "toggle-clips" | "cancel";

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
  /** Browser source only: the site that plays (`youtube`, `yandex`, …), or "". */
  site?: string;
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

// ---------- TTS studio & live speech ----------

export interface TtsSpeakOptions {
  voice?: string | null;
  rate?: number | null;
  interrupt?: boolean | null;
}

export interface TtsSpeakOutcome {
  started: boolean;
  voice: string;
}

export interface TtsStatePayload {
  speaking: boolean;
  text: string;
  voice: string;
  latencyMs: number | null;
}

export interface TtsBenchResult {
  voice: string;
  firstChunkMs: number;
  totalMs: number;
  samples: number;
  cachedMs: number;
}

// ---------- Notch Hub (Tasks, Focus, Shelf/Tray, Activity, Routines) ----------

export type TaskColor = "blue" | "amber" | "coral" | "emerald" | "violet";

export interface TaskItem {
  id: string;
  title: string;
  notes: string;
  scheduledDate: string | null;
  scheduledTime: string | null;
  color: TaskColor | string;
  completed: boolean;
  completedAt: string | null;
  createdAt: string;
}

export interface FocusState {
  active: boolean;
  paused: boolean;
  taskId: string | null;
  title: string;
  totalSec: number;
  remainingSec: number;
  startedAt: string | null;
  endsAt: string | null;
  completedToday: number;
  minutesToday: number;
}

export type ShelfKind = "file" | "text" | "url";

export interface ShelfItem {
  id: string;
  kind: ShelfKind | string;
  label: string;
  value: string;
  subtitle: string;
  sizeBytes: number | null;
  exists: boolean;
  pinned: boolean;
  createdAt: string;
}

export interface ActivityDay {
  date: string;
  count: number;
  level: 0 | 1 | 2 | 3 | 4 | number;
  focusMinutes: number;
  tasksDone: number;
  agentActions: number;
  gitCommits: number;
}

export interface ActivityHeatmap {
  userName: string;
  totalYear: number;
  currentStreak: number;
  longestStreak: number;
  days: ActivityDay[];
}

/** One day of the user's GitHub contribution calendar. */
export interface GithubDay {
  date: string;
  count: number;
  level: number;
}

export interface GithubActivity {
  login: string;
  name: string;
  /** data: URL of the avatar, or null. */
  avatar: string | null;
  profileUrl: string;
  total: number;
  currentStreak: number;
  longestStreak: number;
  days: GithubDay[];
  /** "gh" — GraphQL through the signed-in GitHub CLI (private included); "public" — the public profile. */
  source: "gh" | "public" | string;
  fetchedAt: string;
}

export interface GithubState {
  login: string | null;
  activity: GithubActivity | null;
  error: string | null;
}

export interface RoutineItem {
  id: string;
  title: string;
  subtitle: string;
  icon: string;
  prompt: string;
  autonomy: boolean;
}

export interface RoutineSaveInput {
  id?: string | null;
  title: string;
  subtitle: string;
  icon: string;
  prompt: string;
  autonomy: boolean;
}

export interface MissionPlanStep {
  id: string;
  title: string;
  status: "pending" | "running" | "done" | "failed" | string;
}
