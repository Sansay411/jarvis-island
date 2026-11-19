// Weather on the island: the card JARVIS shows when it looks the weather up
// (`weather:show` from the `weather` tool) and the line under the overview
// clock. The data is real (Open-Meteo through `geo.rs`); nothing is cached here
// beyond what is on screen.

import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import { ipc, isTauri } from "../lib/ipc";

export type WeatherKind = "clear" | "partly" | "cloud" | "fog" | "rain" | "snow" | "storm";

export interface WeatherHour {
  time: string;
  temp: number;
  kind: WeatherKind;
  precip: number;
}

export interface WeatherDay {
  max: number;
  min: number;
  kind: WeatherKind;
  text: string;
  precip: number;
  /** Strongest wind of the day, m/s. */
  wind?: number;
  /** «08:12», empty when unknown. */
  sunrise?: string;
  sunset?: string;
}

export interface Weather {
  place: { city: string; region: string; country: string; lat: number; lon: number; approximate: boolean };
  temp: number;
  feels: number;
  kind: WeatherKind;
  text: string;
  isDay: boolean;
  wind: number;
  humidity: number;
  hours: WeatherHour[];
  today: WeatherDay;
  tomorrow: WeatherDay | null;
  /** Tomorrow 06…21 every three hours. */
  tomorrowHours?: WeatherHour[];
  /** On the card only: the day the answer was about. */
  focus?: "today" | "tomorrow";
}

/** How long the card stays open by itself. */
const CARD_MS = 11000;

interface WeatherStore {
  /** The card on the island, until `cardUntil`. */
  card: Weather | null;
  /** The latest weather where the user is (overview line). */
  here: Weather | null;
  loadedAt: number;
  show: (w: Weather) => void;
  dismiss: () => void;
  loadHere: () => Promise<void>;
}

let hideTimer: number | undefined;

export const useWeather = create<WeatherStore>((set, get) => ({
  card: null,
  here: null,
  loadedAt: 0,
  show: (w) => {
    window.clearTimeout(hideTimer);
    set((s) => ({ card: w, here: w.place.approximate ? { ...w, focus: undefined } : s.here }));
    hideTimer = window.setTimeout(() => set({ card: null }), CARD_MS);
  },
  dismiss: () => {
    window.clearTimeout(hideTimer);
    set({ card: null });
  },
  loadHere: async () => {
    if (!isTauri() || Date.now() - get().loadedAt < 10 * 60_000) return;
    set({ loadedAt: Date.now() });
    try {
      set({ here: await ipc.weather.get() });
    } catch {
      set({ loadedAt: 0 });
    }
  },
}));

if (isTauri()) {
  void listen<Weather>("weather:show", (e) => useWeather.getState().show(e.payload));
}

/** «−7°» */
export function deg(t: number): string {
  const r = Math.round(t);
  return r < 0 ? `−${-r}°` : `${r}°`;
}
