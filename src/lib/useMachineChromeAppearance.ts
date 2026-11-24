import { useCallback, useContext, useEffect, useSyncExternalStore } from "react";
import { readMachineAppearance, type MachineAppearance } from "./chatWallpaperClient";
import { DEFAULT_THEME } from "./themeCss";
import { MachineAppearanceContext } from "./useMachineAppearance";

type Palette = Pick<MachineAppearance["theme"], "name" | "blue" | "background" | "foreground">;
type Entry = {
  appearance?: MachineAppearance;
  listeners: Set<() => void>;
  online: number;
  request?: AbortController;
};

const entries = new Map<string | undefined, Entry>();
const CACHE_KEY = "codeterm-machine-palettes-v1";
const CACHE_LIMIT = 20;
const hex = /^#[0-9a-f]{6}$/i;
let palettes: Map<string, Palette> | undefined;

function isPalette(theme: Palette | undefined): theme is Palette {
  return typeof theme?.name === "string"
    && [theme.blue, theme.background, theme.foreground].every(color => typeof color === "string" && hex.test(color));
}

function cachedPalettes() {
  if (palettes) return palettes;
  palettes = new Map();
  try {
    const raw = localStorage.getItem(CACHE_KEY);
    if (!raw || raw.length > 8192) return palettes;
    for (const [id, palette] of JSON.parse(raw).slice(0, CACHE_LIMIT) as [string, Palette][]) {
      if (typeof id === "string" && isPalette(palette)) {
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

function publishAppearance(peerId: string | undefined, entry: Entry, appearance: MachineAppearance) {
  if (!isPalette(appearance?.theme)) return;
  if (entry.appearance?.theme === appearance.theme && entry.appearance.wallpaper === appearance.wallpaper) return;
  entry.appearance = appearance;
  if (peerId) rememberPalette(peerId, appearance.theme);
  entry.listeners.forEach(listener => listener());
}

function readAppearance(peerId: string | undefined, entry: Entry) {
  if (entry.request) return;
  const controller = new AbortController();
  entry.request = controller;
  void Promise.resolve().then(async () => {
    if (controller.signal.aborted) return;
    const appearance = await readMachineAppearance(peerId, controller.signal);
    if (!controller.signal.aborted) publishAppearance(peerId, entry, appearance);
  }).catch(() => undefined).finally(() => {
    if (entry.request === controller) entry.request = undefined;
  });
}

export function useMachineChromeAppearance(peerId: string | undefined, online: boolean, enabled = true) {
  const selected = useContext(MachineAppearanceContext);
  const projected = enabled && online && selected?.ready && selected.peerId === peerId
    && isPalette(selected.appearance.theme) ? selected.appearance : undefined;
  const theme = projected?.theme;
  const wallpaper = projected?.wallpaper;
  const needsRead = online && !projected;
  useEffect(() => {
    if (theme) publishAppearance(peerId, entryFor(peerId), { theme, wallpaper: wallpaper ?? null });
  }, [peerId, theme, wallpaper]);

  const subscribe = useCallback((listener: () => void) => {
    if (!enabled) return () => undefined;
    const entry = entryFor(peerId);
    entry.listeners.add(listener);
    if (needsRead) {
      entry.online++;
      readAppearance(peerId, entry);
    }
    return () => {
      entry.listeners.delete(listener);
      if (needsRead && --entry.online === 0) {
        entry.request?.abort();
        entry.request = undefined;
      }
    };
  }, [enabled, needsRead, peerId]);
  const cached = useSyncExternalStore(subscribe, () => enabled ? entryFor(peerId).appearance : undefined, () => undefined);
  return projected ?? cached;
}
