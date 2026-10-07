export interface ProviderCliStatus {
  required: boolean;
  installed: boolean | null;
  binary: string | null;
  resolved_path: string | null;
  availability: "available" | "unavailable" | "unknown";
  searched: string[];
  installer: string | null;
  updater: string | null;
}

export function providerCliPresentation(status: ProviderCliStatus | null) {
  if (!status) return { state: "checking", label: "Checking CLI", canInstall: false } as const;
  if (!status.required) return { state: "none", label: "No CLI required", canInstall: false } as const;
  const state = status.installed === true ? "installed" : status.installed === false ? "missing" : "unknown";
  const label = state === "installed" ? "CLI installed" : state === "missing" ? `Missing CLI: ${status.binary}` : "CLI availability unknown";
  return { state, label, canInstall: status.installed !== true && Boolean(status.installer) };
}
