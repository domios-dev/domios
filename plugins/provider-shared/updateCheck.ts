export interface UpdateHost {
  execStart: (opts: string) => string;
  fetchStart: (opts: string) => string;
  awaitJob: <T>(jobId: string, then: (result: unknown) => T) => T;
}

export interface CliCompatibility {
  supported: string[];
  breaking?: { range: string }[];
}

export interface UpdateAvailability {
  currentVersion: string;
  latestVersion: string;
}

function record(raw: unknown): Record<string, unknown> | null {
  if (raw !== null && typeof raw === "object") return raw as Record<string, unknown>;
  if (typeof raw !== "string") return null;
  try {
    const value: unknown = JSON.parse(raw);
    return value !== null && typeof value === "object" ? value as Record<string, unknown> : null;
  } catch { return null; }
}

function versionParts(version: string): number[] | null {
  if (!/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(version)) return null;
  const parts = version.split(".").map(Number);
  return parts.every(Number.isSafeInteger) ? parts : null;
}

function compare(left: string, right: string): number | null {
  const a = versionParts(left), b = versionParts(right);
  if (!a || !b) return null;
  for (let index = 0; index < 3; index++) {
    if (a[index] !== b[index]) return a[index] < b[index] ? -1 : 1;
  }
  return 0;
}

export function matchesRange(version: string, range: string): boolean {
  return range.split(",").every(clause => {
    const match = clause.trim().match(/^(>=|<=|>|<|=)?\s*(\d+\.\d+\.\d+)$/);
    if (!match) return false;
    const order = compare(version, match[2]);
    if (order === null) return false;
    switch (match[1] ?? "=") {
      case ">=": return order >= 0;
      case "<=": return order <= 0;
      case ">": return order > 0;
      case "<": return order < 0;
      default: return order === 0;
    }
  });
}

export function newestSupportedVersion(versions: string[], compatibility: CliCompatibility): string | null {
  return versions.reduce<string | null>((best, version) => {
    if (!versionParts(version)
      || !compatibility.supported.some(range => matchesRange(version, range))
      || compatibility.breaking?.some(rule => matchesRange(version, rule.range))) return best;
    return best === null || compare(version, best) === 1 ? version : best;
  }, null);
}

export function installedVersion(result: unknown): string | null {
  const installed = record(result);
  if (installed?.code !== 0 || typeof installed.stdout !== "string") return null;
  const version = installed.stdout.match(/(?:^|\s)(\d+\.\d+\.\d+)(?=\s|$)/)?.[1];
  return version && versionParts(version) ? version : null;
}

export function availableUpdate(
  currentVersion: string,
  result: unknown,
  compatibility?: CliCompatibility,
): UpdateAvailability | null {
  const response = record(result);
  if (response?.status !== 200) return null;
  const metadata = record(response.body);
  let latestVersion: string | null = typeof metadata?.version === "string" ? metadata.version : null;
  if (!compatibility && !latestVersion) return null;
  if (compatibility) {
    if (!metadata?.versions || typeof metadata.versions !== "object") return null;
    latestVersion = newestSupportedVersion(Object.keys(metadata.versions), compatibility);
  }
  // An answered check with no newer target reports latest = current, so the host can tell it from no answer (null).
  return latestVersion && compare(latestVersion, currentVersion) === 1
    ? { currentVersion, latestVersion } : { currentVersion, latestVersion: currentVersion };
}

function startedJob(raw: string): string | null {
  const job = record(raw)?.jobId;
  return typeof job === "string" && job !== "" ? job : null;
}

// Each probe yields the plugin VM through awaitJob, so the VM is held only to start a job and read its answer.
export function checkProviderUpdate(
  host: UpdateHost,
  binary: string,
  metadataUrl: string,
  compatibility?: CliCompatibility,
  parseInstalled: (result: unknown) => string | null = installedVersion,
): UpdateAvailability | null {
  const probe = startedJob(host.execStart(JSON.stringify({ bin: binary, args: ["--version"], timeoutMs: 5000 })));
  if (!probe) return null;
  return host.awaitJob(probe, installed => {
    const currentVersion = parseInstalled(installed);
    if (!currentVersion) return null;
    const metadata = startedJob(host.fetchStart(JSON.stringify({ url: metadataUrl, method: "GET", timeoutMs: 6000,
      headers: { Accept: "application/vnd.npm.install-v1+json" },
    })));
    if (!metadata) return null;
    return host.awaitJob(metadata, response => availableUpdate(currentVersion, response, compatibility));
  });
}
