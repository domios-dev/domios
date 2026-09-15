/**
 * Bundled plugin loader.
 *
 * Discovers every `plugins/<id>/src/index.ts` workspace package via
 * Vite's `import.meta.glob` and registers each plugin's default
 * export. Runs once at app start; Vite HMR re-runs individual plugin
 * modules on edit, which calls `register()` again to swap function
 * refs in-place. No daemon restart needed.
 *
 * For now this only loads in-repo bundled plugins. Loading user-
 * installed plugins from `~/.codeterm/plugins/<id>/dist/plugin.js`
 * is a follow-up
 * and will use a parallel discovery path that reads via tauri's
 * filesystem APIs.
 */
import { register } from "./registry";
import type { ProviderPlugin } from "./types";

// mounts so no fallback flash happens. Pattern matches every workspace plugin's entry point. Adding a new `plugins/provider-<x>/src/index.ts` is zero-touch for the loader.
const modules = import.meta.glob<{ default: ProviderPlugin }>(
  "/plugins/*/src/index.ts",
  { eager: true },
);

let loaded = false;

export function loadBundledPlugins(): void {
  if (loaded) return;
  loaded = true;
  for (const [path, mod] of Object.entries(modules)) {
    const plugin = mod.default;
    if (!plugin?.meta?.id) {
      console.warn(`[plugins] ${path} has no default export with meta.id — skipped`);
      continue;
    }
    register(plugin);
  }
  if ((import.meta as { env?: { DEV?: boolean } }).env?.DEV) {
    console.debug(`[plugins] bundled load complete (${Object.keys(modules).length} plugins)`);
  }
}

// In dev, HMR re-runs this module when a plugin module changes. We re-register everything; consumers re-render via `useRegistryVersion`.
if (import.meta.hot) {
  import.meta.hot.accept(() => {
    loaded = false;
    loadBundledPlugins();
  });
}
