// Background jobs (coding agents etc.) pushed by the native job manager.

import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export interface JobView {
  id: string;
  kind: string;
  title: string;
  cwd: string;
  status: "running" | "succeeded" | "failed" | "cancelled";
  startedAt: string;
  finishedAt: string | null;
  exitCode: number | null;
  finalText: string | null;
  error: string | null;
  tail: string[];
}

export const useJobs = create<{ jobs: JobView[] }>(() => ({ jobs: [] }));

export async function startJobsFeed() {
  await listen<JobView[]>("jobs:update", (e) => useJobs.setState({ jobs: e.payload }));
  try {
    useJobs.setState({ jobs: await invoke<JobView[]>("jobs_list") });
  } catch {
    /* no jobs yet */
  }
}

export const cancelJob = (id: string) => invoke("job_cancel", { id });

export function useRunningJobs(): JobView[] {
  return useJobs((s) => s.jobs).filter((j) => j.status === "running");
}
