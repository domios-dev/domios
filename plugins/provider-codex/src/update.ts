export interface UpdateHost {
  exec: (opts: string) => string;
  fetch: (opts: string) => string;
}

export interface UpdateAvailability {
  currentVersion: string;
  latestVersion: string;
}

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
  if (typeof value !== "string") return null;
  try {
    const parsed: unknown = JSON.parse(value);
    return parsed && typeof parsed === "object" ? parsed as Record<string, unknown> : null;
  } catch {
    return null;
  }
}

export function checkCodexUpdate(host: UpdateHost): UpdateAvailability | null {
  const installed = record(host.exec(JSON.stringify({
    bin: "codex",
    args: ["--version"],
    timeoutMs: 5000,
  })));
  if (installed?.code !== 0 || typeof installed.stdout !== "string") return null;
  const current = installed.stdout.match(/\d+\.\d+\.\d+/)?.[0];
  if (!current) return null;
  const response = record(host.fetch(JSON.stringify({
    url: "https://registry.npmjs.org/@openai%2Fcodex/latest",
    method: "GET",
    timeoutMs: 6000,
  })));
  if (response?.status !== 200 || typeof response.body !== "string") return null;
  const metadata = record(response.body);
  const latest = metadata?.version;
  return updateAvailable(current, latest) && typeof latest === "string"
    ? { currentVersion: current, latestVersion: latest }
    : null;
}
