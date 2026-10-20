import { checkProviderUpdate, installedVersion, type UpdateAvailability, type UpdateHost } from "../../provider-shared/updateCheck";

export const codexSelfUpdateCapability = {
  npmPackage: "@openai/codex",
  standaloneDirectory: "packages/standalone",
  standaloneArgs: ["update"],
};

export function parseVersion(value: unknown): [number, number, number] | null {
  if (typeof value !== "string") return null;
  const match = value.match(/^(?:codex(?:-cli)?\s+)?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)\s*$/);
  if (!match) return null;
  const parts: [number, number, number] = [Number(match[1]), Number(match[2]), Number(match[3])];
  return parts.every(Number.isSafeInteger) ? parts : null;
}

export function updateAvailable(current: unknown, latest: unknown): boolean {
  const installed = parseVersion(current);
  const offered = parseVersion(latest);
  if (!installed || !offered) return false;
  for (let i = 0; i < 3; i += 1) {
    if (offered[i] !== installed[i]) return offered[i] > installed[i];
  }
  return false;
}

export function checkCodexUpdate(host: UpdateHost): UpdateAvailability | null {
  return checkProviderUpdate(host, "codex", `https://registry.npmjs.org/${encodeURIComponent(codexSelfUpdateCapability.npmPackage)}/latest`, undefined,
    installedVersion);
}
