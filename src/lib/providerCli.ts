export interface ProviderCliStatus {
  detected_version?: string | null;
  version_probe?: "pending" | "detected" | "missing" | "unreadable" | "failed" | "timeout" | "invalid" | "saturated";
  compatibility?: { verdict: "supported" | "breaking" | "unknown"; reasons: string[] };
  required: boolean;
  installed: boolean | null;
  binary: string | null;
  resolved_path: string | null;
  availability: "available" | "unavailable" | "unknown";
  searched: string[];
  installer: string | null;
  updater: string | null;
  installer_prerequisites?: { binary: string; installed: boolean | null; hint: string }[];
  updater_prerequisites?: { binary: string; installed: boolean | null; hint: string }[];
}

export function missingInstallerPrerequisites(status: ProviderCliStatus | null) {
  return status?.installer_prerequisites?.filter(item => item.installed === false) ?? [];
}

export function providerCliPresentation(status: ProviderCliStatus | null) {
  if (!status) return { state: "checking", label: "Checking CLI", canInstall: false } as const;
  if (!status.required) return { state: "none", label: "No CLI required", canInstall: false } as const;
  const state = status.installed === true ? "installed" : status.installed === false ? "missing" : "unknown";
  const label = state === "installed" ? "CLI installed" : state === "missing" ? `Missing CLI: ${status.binary}` : "CLI availability unknown";
  return { state, label, canInstall: status.installed !== true && Boolean(status.installer) };
}
