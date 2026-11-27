export type ProviderCliStatus = Pick<import("../../gen/ProviderCliStatus").ProviderCliStatus, "required" | "installed" | "binary" | "resolved_path" | "availability" | "searched" | "installer" | "updater"> & Partial<Omit<import("../../gen/ProviderCliStatus").ProviderCliStatus, "update_job">> & { update_job?: (Omit<NonNullable<import("../../gen/ProviderCliStatus").ProviderCliStatus["update_job"]>, "state"> & { state: "running" | "succeeded" | "failed" }) | null };

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

export interface PrerequisiteGuidance {
  binary: string;
  hint: string;
  installUrl: string | null;
  installHostPath: string | null;
  remediationCommand: string | null;
  remediationLanguage: string;
}

export function missingInstallerPrerequisites(status: ProviderCliStatus | null) {
  return status?.installer_prerequisites?.filter(item => item.installed === false) ?? [];
}

export function prerequisiteGuidance(item: { binary: string; hint: string }): PrerequisiteGuidance {
  let installUrl: string | null = null;
  let installHostPath: string | null = null;
  const rawUrl = item.hint.match(/https:\/\/[^\s,;)\]"']+/)?.[0]?.replace(/[.,;:!?)]+$/, "");
  if (rawUrl) {
    try {
      const parsed = new URL(rawUrl);
      if (parsed.protocol === "https:") {
        installUrl = parsed.toString();
        const trimmedPath = parsed.pathname.replace(/\/+$/, "");
        installHostPath = `${parsed.host}${trimmedPath}`;
      }
    } catch {
      installUrl = null;
    }
  }
  const remediationCommand =
    item.hint.match(/Set-ExecutionPolicy\s+-Scope\s+\S+\s+\S+/i)?.[0] ?? null;
  return {
    binary: item.binary,
    hint: item.hint,
    installUrl,
    installHostPath,
    remediationCommand,
    remediationLanguage: remediationCommand ? "powershell" : "bash",
  };
}

export function installerShellLanguage(command: string): string {
  return /\b(iex|irm|iwr|Get-Command|Invoke-WebRequest|Invoke-RestMethod|Invoke-Expression|powershell|pwsh)\b/i.test(command) ? "powershell" : "bash";
}

export function providerCliPresentation(status: ProviderCliStatus | null): { state: ProviderCliState; label: string; canInstall: boolean } {
  if (!status) return { state: "checking", label: "Checking CLI", canInstall: false };
  if (status.launch_profiles) {
    const ready = status.launch_profiles.filter(profile => profile.installed === true).length;
    const state = status.installed === true ? "installed" : status.installed === false ? "missing" : "unknown";
    const label = ready ? `${ready} of ${status.launch_profiles.length} launch profiles ready`
      : state === "unknown" ? "Launch profile readiness unknown" : "No launch profiles ready";
    return { state, label, canInstall: false };
  }
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
    label: state === "unknown" && !status?.launch_profiles ? "CLI status unknown" : base.label,
    version: state === "installed" ? status?.detected_version?.trim() || null : null,
    pathHint: state === "installed" ? status?.resolved_path ?? null : null,
    canInstall: base.canInstall,
    canRecheck: Boolean(status?.launch_profiles) || state === "unknown",
    installCommand: missing ? installer : null,
    installLanguage: installer ? installerShellLanguage(installer) : "bash",
    prerequisites: missing ? missingInstallerPrerequisites(status).map(({ binary, hint }) => ({ binary, hint })) : [],
  };
}
