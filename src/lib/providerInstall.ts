import { invoke } from "./bridge";

export interface ProviderInstallLaunch {
  tab_id: string;
  launch_state: string;
}

export function requireProviderInstallLaunch(
  result: ProviderInstallLaunch | null | undefined,
): ProviderInstallLaunch {
  if (!result?.tab_id) throw new Error("Installer launch returned no tab id");
  if (result.launch_state !== "command_dispatched") {
    throw new Error("Installer returned without a command-dispatch acknowledgement");
  }
  return result;
}

/**
 * What a picker shows beneath an Install button. The shell plumbing is noise;
 * the source the code comes from is the part worth reading before you click.
 */
export function installSource(command: string | null | undefined): string | null {
  if (!command) return null;
  const url = command.match(/https?:\/\/[^\s'"]+/)?.[0];
  if (!url) return command.trim() || null;
  return url.replace(/^https?:\/\//, "").replace(/\?.*$/, "").replace(/\/$/, "");
}

export async function launchProviderInstaller(providerId: string): Promise<ProviderInstallLaunch> {
  const result = await invoke<ProviderInstallLaunch>("install_provider_in_tab", { providerId });
  return requireProviderInstallLaunch(result);
}
