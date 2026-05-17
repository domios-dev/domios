/**
 * Plugin registry — the host-side store of loaded ProviderPlugin
 * objects. Plugins register themselves at app startup via the loader
 * (see `loader.ts`); the rest of the UI looks them up by provider id.
 *
 * Hot-reload friendly: when Vite HMR re-imports a plugin module, the
 * loader calls `register()` again with the same id, which overwrites
 * the old reference. Any consumer that re-renders (subscribes to
 * `useRegistryVersion()`) picks up the new functions automatically.
 */
import { useEffect, useState } from "react";

import type { ProviderPlugin } from "./types";

type Listener = () => void;

const plugins = new Map<string, ProviderPlugin>();
const listeners = new Set<Listener>();
let version = 0;

function bump() {
  version += 1;
  for (const l of listeners) l();
}

/** Register / replace a plugin by id. Idempotent. */
export function register(plugin: ProviderPlugin): void {
  plugins.set(plugin.meta.id, plugin);
  bump();
  if (import.meta.env.DEV) {
    console.debug(`[plugins] registered ${plugin.meta.id} (v${version})`);
  }
}

/** Unregister a plugin (for hot-reload teardown / tests). */
export function unregister(id: string): void {
  if (plugins.delete(id)) bump();
}

/** Lookup by id. Returns undefined for unknown providers — callers
 *  must fall back to the raw xterm renderer. */
export function getPlugin(id: string | null | undefined): ProviderPlugin | undefined {
  if (!id) return undefined;
  return plugins.get(id);
}

/** All registered plugins. Stable order = insertion order. */
export function getAllPlugins(): ProviderPlugin[] {
  return Array.from(plugins.values());
}

/** Detect provider from PTY bytes + optional title. Returns the first
 *  matching plugin id, or undefined. Cheap — pure function calls. */
export function detectProvider(bytes: string, title?: string): string | undefined {
  const sample = { bytes, title };
  for (const p of plugins.values()) {
    if (p.detect(sample)) return p.meta.id;
    if (title && p.titlePattern?.test(title)) return p.meta.id;
  }
  return undefined;
}

/** Pure helper for old-shape consumers (`isProviderNoise`). */
export function isProviderNoise(id: string | null | undefined, text: string): boolean {
  const p = getPlugin(id);
  if (!p?.noisePatterns) return false;
  const t = text.trim();
  if (!t) return true;
  return p.noisePatterns.some(rx => rx.test(t));
}

/** React hook — bumps when any plugin is (un)registered. Wrap
 *  components that depend on the registry to re-render on hot reload. */
export function useRegistryVersion(): number {
  const [v, setV] = useState(version);
  useEffect(() => {
    const l = () => setV(version);
    listeners.add(l);
    return () => {
      listeners.delete(l);
    };
  }, []);
  return v;
}
