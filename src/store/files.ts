// Files dropped on the island: their facts and actions come from the native
// side (`files_inspect`); actions run there as jobs reported by `files:job`.

import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { errorMessage } from "../lib/ipc";
import { useIsland } from "./island";
import { useAgent } from "./agent";

export type FileKind = "image" | "video" | "audio" | "pdf" | "document" | "spreadsheet" | "presentation" | "archive" | "text" | "code" | "folder" | "other";

export interface FileInfo {
  path: string;
  name: string;
  ext: string;
  kind: FileKind;
  size: number;
  width: number | null;
  height: number | null;
  durationS: number | null;
  pages: number | null;
  videoCodec: string | null;
  audioCodec: string | null;
  thumbnail: string | null;
}

export interface FileAction {
  id: string;
  label: string;
  hint: string | null;
  primary: boolean;
}

export interface FileJob {
  jobId: string;
  action: string;
  title: string;
  state: "running" | "done" | "failed" | "cancelled";
  progress: number | null;
  message: string | null;
  outputs: string[];
  text: string | null;
}

interface FilesStore {
  open: boolean;
  loading: boolean;
  error: string | null;
  paths: string[];
  files: FileInfo[];
  actions: FileAction[];
  /** The latest action on these files. */
  job: FileJob | null;
  /** Measured height of the card (px). */
  height: number;
}

export const useFiles = create<FilesStore>(() => ({ open: false, loading: false, error: null, paths: [], files: [], actions: [], job: null, height: 0 }));

let seq = 0;
// A fast action can finish before `files_action` returns its id.
const latest = new Map<string, FileJob>();

export async function showFiles(paths: string[]) {
  const my = ++seq;
  useFiles.setState({ open: true, loading: true, error: null, paths, files: [], actions: [], job: null });
  try {
    const r = await invoke<{ files: FileInfo[]; actions: FileAction[] }>("files_inspect", { paths });
    if (my === seq) useFiles.setState({ loading: false, files: r.files, actions: r.actions });
  } catch (e) {
    if (my === seq) useFiles.setState({ loading: false, error: errorMessage(e) });
  }
}

export function closeFiles() {
  seq++;
  void invoke("files_clear");
  useFiles.setState({ open: false, loading: false, error: null, paths: [], files: [], actions: [], job: null });
}

const ASK: Record<string, string> = {
  "ask:summarize": "Кратко перескажи, о чём этот файл, и выдели главное.",
  "ask:translate": "Переведи содержимое файла на русский (если он уже на русском — на английский).",
};

/** Attach the files to the chat (and optionally ask right away). */
function toChat(paths: string[], ask?: string) {
  const island = useIsland.getState();
  island.addAttachments(paths.map((path) => ({ path, name: path.split(/[\\/]/).pop() ?? path })));
  if (ask) {
    void useAgent.getState().send(ask, paths);
    island.set({ pendingAttachments: [] });
  }
  closeFiles();
  island.set({ workspaceOpen: true });
}

export async function runFileAction(id: string, paths = useFiles.getState().paths) {
  if (id === "chat") return toChat(paths);
  if (ASK[id]) return toChat(paths, ASK[id]);
  if (id === "need_ffmpeg") {
    useFiles.setState({ job: { jobId: "", action: id, title: "Нужен FFmpeg", state: "failed", progress: null, message: "Установите FFmpeg: winget install Gyan.FFmpeg — и перетащите файл снова.", outputs: [], text: null } });
    return;
  }
  try {
    const jobId = await invoke<string>("files_action", { paths, action: id });
    useFiles.setState({ job: latest.get(jobId) ?? { jobId, action: id, title: "", state: "running", progress: 0, message: null, outputs: [], text: null } });
  } catch (e) {
    useFiles.setState({ job: { jobId: "", action: id, title: "", state: "failed", progress: null, message: errorMessage(e), outputs: [], text: null } });
  }
}

export function cancelFileJob() {
  const job = useFiles.getState().job;
  if (job?.jobId && job.state === "running") void invoke("files_cancel", { jobId: job.jobId });
}

/** Results are files too: open, show, send or attach them. */
export function resultAction(id: "open" | "reveal" | "telegram" | "chat", outputs: string[]) {
  if (id === "chat") return toChat(outputs);
  if (id === "telegram") return void runFileAction(id, outputs);
  void invoke("files_action", { paths: outputs, action: id });
}

export async function startFilesFeed() {
  await listen<FileJob>("files:job", (e) => {
    latest.set(e.payload.jobId, e.payload);
    if (latest.size > 50) latest.delete(latest.keys().next().value!);
    const job = useFiles.getState().job;
    if (job && job.jobId === e.payload.jobId) useFiles.setState({ job: e.payload });
  });
}
