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

export async function launchProviderInstaller(providerId: string): Promise<ProviderInstallLaunch> {
  const result = await invoke<ProviderInstallLaunch>("install_provider_in_tab", { providerId });
  return requireProviderInstallLaunch(result);
}
