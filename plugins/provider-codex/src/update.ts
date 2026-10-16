import { checkProviderUpdate, type UpdateAvailability, type UpdateHost } from "../../provider-shared/updateCheck";

export function parseVersion(value: unknown): [number, number, number] | null {
  if (typeof value !== "string") return null;
  const match = value.match(/(?:^|\s)(\d+)\.(\d+)\.(\d+)(?:\s|$)/);
  if (!match) return null;
  return [Number(match[1]), Number(match[2]), Number(match[3])];
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

function record(value: unknown): Record<string, unknown> | null {
  if (value !== null && typeof value === "object") return value as Record<string, unknown>;
  if (typeof value !== "string") return null;
  try {
    const parsed: unknown = JSON.parse(value);
    return parsed && typeof parsed === "object" ? parsed as Record<string, unknown> : null;
  } catch {
    return null;
  }
}

function installedCodexVersion(result: unknown): string | null {
  const installed = record(result);
  if (installed?.code !== 0 || typeof installed.stdout !== "string") return null;
  const current = installed.stdout.match(/\d+\.\d+\.\d+/)?.[0];
  return current && parseVersion(current) ? current : null;
}

export function checkCodexUpdate(host: UpdateHost): UpdateAvailability | null {
  return checkProviderUpdate(host, "codex", "https://registry.npmjs.org/@openai%2Fcodex/latest", undefined,
    installedCodexVersion);
}
