import { useCallback, useSyncExternalStore } from "react";
import { readMachineAppearance, type MachineAppearance } from "./chatWallpaperClient";
import { daemonTransportForPeer } from "./daemonBootstrap";
import { DEFAULT_THEME } from "./themeCss";
import type { AppConfig } from "./types";

type Palette = Pick<MachineAppearance["theme"], "name" | "blue" | "background" | "foreground">;
type Entry = {
  appearance?: MachineAppearance;
  listeners: Set<() => void>;
  online: number;
  stop?: () => void;
};

const entries = new Map<string | undefined, Entry>();
const CACHE_KEY = "codeterm-machine-palettes-v1";
const CACHE_LIMIT = 20;
const hex = /^#[0-9a-f]{6}$/i;
let palettes: Map<string, Palette> | undefined;

function cachedPalettes() {
  if (palettes) return palettes;
  palettes = new Map();
  try {
    const raw = localStorage.getItem(CACHE_KEY);
    if (!raw || raw.length > 8192) return palettes;
    for (const [id, palette] of JSON.parse(raw).slice(0, CACHE_LIMIT) as [string, Palette][]) {
      if (typeof id === "string" && typeof palette?.name === "string"
          && [palette.blue, palette.background, palette.foreground].every(color => hex.test(color))) {
        palettes.set(id, palette);
      }
    }
  } catch {
    // The last live appearance is still available when storage is disabled.
  }
  return palettes;
}

function rememberPalette(peerId: string, { name, blue, background, foreground }: Palette) {
  const cache = cachedPalettes();
  const previous = cache.get(peerId);
  if (previous?.name === name && previous.blue === blue && previous.background === background
      && previous.foreground === foreground) return;
  cache.delete(peerId);
  cache.set(peerId, { name: name.slice(0, 128), blue, background, foreground });
  if (cache.size > CACHE_LIMIT) cache.delete(cache.keys().next().value!);
  try { localStorage.setItem(CACHE_KEY, JSON.stringify([...cache])); } catch { /* best effort */ }
}

function entryFor(peerId: string | undefined) {
  let entry = entries.get(peerId);
  if (!entry) {
    const palette = peerId ? cachedPalettes().get(peerId) : undefined;
    entry = {
      appearance: palette && { theme: { ...DEFAULT_THEME, ...palette }, wallpaper: null },
      listeners: new Set(),
      online: 0,
    };
    entries.set(peerId, entry);
  }
  return entry;
}

function observeAppearance(peerId: string | undefined, entry: Entry) {
  const controller = new AbortController();
  let changed = false;
  let unsubscribe: (() => void) | undefined;
  const publish = (appearance: MachineAppearance) => {
    if (controller.signal.aborted) return;
    entry.appearance = appearance;
    if (peerId) rememberPalette(peerId, appearance.theme);
    entry.listeners.forEach(listener => listener());
  };
  void Promise.resolve().then(async () => {
    if (controller.signal.aborted) return;
    unsubscribe = daemonTransportForPeer(peerId).subscribe("/api/events/stream", (name, data) => {
      if (name !== "config-changed" || !data || typeof data !== "object" || !("theme" in data) || !data.theme) return;
      const config = data as AppConfig;
      changed = true;
      publish({ theme: config.theme, wallpaper: config.chat_wallpaper ?? null });
    }, { signal: controller.signal, onEnd: () => undefined });
    const appearance = await readMachineAppearance(peerId, controller.signal);
    if (!changed) publish(appearance);
  }).catch(() => undefined);
  return () => { controller.abort(); unsubscribe?.(); };
}

export function useMachineChromeAppearance(peerId: string | undefined, online: boolean, enabled = true) {
  const subscribe = useCallback((listener: () => void) => {
    if (!enabled) return () => undefined;
    const entry = entryFor(peerId);
    entry.listeners.add(listener);
    if (online && entry.online++ === 0) entry.stop = observeAppearance(peerId, entry);
    return () => {
      entry.listeners.delete(listener);
      if (online && --entry.online === 0) {
        entry.stop?.();
        entry.stop = undefined;
      }
    };
  }, [enabled, online, peerId]);
  return useSyncExternalStore(subscribe, () => enabled ? entryFor(peerId).appearance : undefined, () => undefined);
}
