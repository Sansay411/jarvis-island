// Paired Bluetooth devices and their batteries, mirrored from the native
// watcher (`bluetooth:devices`). The «Звук» deck, the brow chip and the short
// «connected» card read it.

import { create } from "zustand";
import { ipc, isTauri, on } from "../lib/ipc";
import type { BtDevice, BtKind } from "../lib/types";

export interface BtHello {
  id: string;
  /** «connected»: the headset just came on; «low»: it fell to 20 % or 10 %. */
  reason: "connected" | "low";
  at: number;
}

interface BluetoothStore {
  devices: BtDevice[];
  /** A headset just connected or ran low: the island says so for a moment. */
  hello: BtHello | null;
  dismissHello: () => void;
}

export const useBluetooth = create<BluetoothStore>((set) => ({
  devices: [],
  hello: null,
  dismissHello: () => set({ hello: null }),
}));

const AUDIO: BtKind[] = ["headphones", "earbuds", "headset", "speaker", "audio"];

export function isAudio(d: BtDevice): boolean {
  return AUDIO.includes(d.kind);
}

/** The lowest bud in use, or the level of the whole device. */
export function levelOf(d: BtDevice): number | null {
  const buds = [d.left, d.right].map((p) => p?.level).filter((l): l is number => l != null);
  if (buds.length > 0) return Math.min(...buds);
  return d.battery;
}

export function isCharging(d: BtDevice): boolean {
  return Boolean(d.left?.charging || d.right?.charging);
}

/** The headphones that matter now: the ones sound plays through, else any connected audio device. */
export function primaryAudio(devices: BtDevice[]): BtDevice | null {
  return devices.find((d) => d.connected && d.audioOutput) ?? devices.find((d) => d.connected && isAudio(d)) ?? null;
}

/** Ring and text tone for a level: calm, getting low, almost empty. */
export function levelTone(level: number | null, charging = false): "ok" | "low" | "critical" | "charging" | "unknown" {
  if (charging) return "charging";
  if (level == null) return "unknown";
  if (level <= 10) return "critical";
  if (level <= 20) return "low";
  return "ok";
}

/** «4 ч 10 мин», «45 мин». */
export function humanMinutes(m: number): string {
  const h = Math.floor(m / 60);
  const min = Math.round(m % 60);
  if (h === 0) return `${min} мин`;
  if (min === 0) return `${h} ч`;
  return `${h} ч ${min} мин`;
}

let loaded = false;

function apply(next: BtDevice[]) {
  const prev = useBluetooth.getState().devices;
  const patch: Partial<BluetoothStore> = { devices: next };
  // Headphones that just came on say hello — not the ones already on at start
  // (the native watcher's first list arrives a few seconds after the window).
  if (loaded && prev.length > 0) {
    const came = next.find((d) => d.connected && isAudio(d) && !prev.find((p) => p.id === d.id)?.connected);
    if (came) patch.hello = { id: came.id, reason: "connected", at: Date.now() };
    const gone = useBluetooth.getState().hello;
    if (gone && !next.find((d) => d.id === gone.id)?.connected) patch.hello = null;
  }
  loaded = true;
  useBluetooth.setState(patch);
}

export async function startBluetoothFeed() {
  if (!isTauri()) return;
  await on("bluetooth:devices", apply);
  await on("bluetooth:low", ({ name }) => {
    const d = useBluetooth.getState().devices.find((x) => x.name === name && x.connected);
    if (d) useBluetooth.setState({ hello: { id: d.id, reason: "low", at: Date.now() } });
  });
  try {
    apply(await ipc.bluetooth.devices());
  } catch {
    /* the watcher pushes the first list a few seconds after start */
  }
}
