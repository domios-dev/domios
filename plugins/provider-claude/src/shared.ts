// Manifest access shared across the Claude provider modules. The host exposes
// the plugin's manifest.json as a string; every module that needs configured
// paths/models parses it through here.
import type { ModelInfo } from "@codeterm/plugin-sdk";

export interface Manifest {
  rule?: { path?: string };
  integration?: { settingsPath?: string };
  sessions?: { rootDir?: string; fileExtension?: string };
  models?: ModelInfo[];
}

export function manifest(): Manifest | null {
  try { return JSON.parse(host.manifestJson() || "null") as Manifest; } catch (e) { return null; }
}
