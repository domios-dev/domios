import type { ModelInfo, ProviderCompatibilityManifest } from "@codeterm/plugin-sdk";

export interface Manifest {
  rule?: { path?: string };
  integration?: { settingsPath?: string };
  sessions?: { rootDir?: string; fileExtension?: string };
  models?: ModelInfo[];
  cliCompatibility?: ProviderCompatibilityManifest;
}

export function manifest(): Manifest | null {
  try { return JSON.parse(host.manifestJson() || "null") as Manifest; } catch (e) { return null; }
}
