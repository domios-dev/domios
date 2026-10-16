import type { ModelInfo, ProviderCompatibilityManifest } from "@codeterm/plugin-sdk";

export interface Manifest {
  rule?: { path?: string };
  integration?: { settingsPath?: string };
  sessions?: { rootDir?: string; fileExtension?: string };
  models?: ModelInfo[];
  commands?: {
    launchSpec?: { program: string; args?: string[]; envUnset?: string[] };
    modelSwitchCommands?: { transcript?: unknown; effort?: { confirmation: string } };
  };
  cliCompatibility?: ProviderCompatibilityManifest;
}

export function manifest(): Manifest | null {
  try { return JSON.parse(host.manifestJson() || "null") as Manifest; } catch (e) { return null; }
}
