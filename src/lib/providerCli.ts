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

export type ProviderCliState = "checking" | "none" | "installed" | "missing" | "unknown";

export interface ProviderCliView {
  state: ProviderCliState;
  label: string;
  version: string | null;
  compatibilityHint: string | null;
  pathHint: string | null;
  canInstall: boolean;
  canRecheck: boolean;
  installCommand: string | null;
  installLanguage: string;
  prerequisites: { binary: string; hint: string }[];
}

export function missingInstallerPrerequisites(status: ProviderCliStatus | null) {
  return status?.installer_prerequisites?.filter(item => item.installed === false) ?? [];
}

export function installerShellLanguage(command: string): string {
  return /\b(iex|irm|Invoke-RestMethod|Invoke-Expression|powershell|pwsh)\b/i.test(command) ? "powershell" : "bash";
}

export function providerCliPresentation(status: ProviderCliStatus | null): { state: ProviderCliState; label: string; canInstall: boolean } {
  if (!status) return { state: "checking", label: "Checking CLI", canInstall: false };
  if (!status.required) return { state: "none", label: "No CLI required", canInstall: false };
  const state: ProviderCliState = status.installed === true ? "installed" : status.installed === false ? "missing" : "unknown";
  const label = state === "installed" ? "CLI installed" : state === "missing" ? "CLI not installed" : "CLI status unknown";
  return { state, label, canInstall: status.installed !== true && Boolean(status.installer) };
}

export function providerCompatibilityHint(status: ProviderCliStatus | null): string | null {
  if (!status?.required || status.installed !== true) return null;
  const compatibility = status.compatibility;
  if (!compatibility || compatibility.verdict === "supported" || compatibility.reasons.includes("undeclared_capability")) return null;
  const version = status.detected_version?.trim();
  if (compatibility.verdict === "breaking") return "CLI version is incompatible; transcript reads are disabled. Use the terminal.";
  return version
    ? `CLI ${version} is not verified yet; transcript reads are best effort. Use the terminal if Chat is unavailable.`
    : "CLI version is unknown; transcript compatibility is not verified. Use the terminal if Chat is unavailable.";
}

export function providerCliView(status: ProviderCliStatus | null, error: string | null): ProviderCliView {
  const base = providerCliPresentation(status);
  const state: ProviderCliState = error && base.state === "checking" ? "unknown" : base.state;
  const missing = state === "missing";
  const installer = status?.installer ?? null;
  return {
    state,
    compatibilityHint: providerCompatibilityHint(status),
    label: state === "unknown" ? "CLI status unknown" : base.label,
    version: state === "installed" ? status?.detected_version?.trim() || null : null,
    pathHint: state === "installed" ? status?.resolved_path ?? null : null,
    canInstall: base.canInstall,
    canRecheck: state === "unknown",
    installCommand: missing ? installer : null,
    installLanguage: installer ? installerShellLanguage(installer) : "bash",
    prerequisites: missing ? missingInstallerPrerequisites(status).map(({ binary, hint }) => ({ binary, hint })) : [],
  };
}
