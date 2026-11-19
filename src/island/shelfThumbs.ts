// Lazy thumbnails for shelf tiles: asked from Rust once per path and kept for
// the life of the island window. The QA harness primes the cache directly.

import { useEffect, useState } from "react";
import { ipc, isTauri } from "../lib/ipc";

const cache = new Map<string, string | null>();
const inflight = new Map<string, Promise<string | null>>();

export function primeShelfThumbs(entries: Record<string, string | null>) {
  for (const [k, v] of Object.entries(entries)) cache.set(k, v);
}

export function useShelfThumb(path: string | null): string | null {
  const [url, setUrl] = useState<string | null>(() => (path ? cache.get(path) ?? null : null));
  useEffect(() => {
    if (!path) return;
    if (cache.has(path)) {
      setUrl(cache.get(path) ?? null);
      return;
    }
    if (!isTauri()) return;
    let alive = true;
    let p = inflight.get(path);
    if (!p) {
      p = ipc.hub.shelfThumbnail(path).catch(() => null);
      inflight.set(path, p);
    }
    void p.then((v) => {
      cache.set(path, v);
      inflight.delete(path);
      if (alive) setUrl(v);
    });
    return () => {
      alive = false;
    };
  }, [path]);
  return url;
}
