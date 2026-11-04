import {useEffect, useState} from "react";
import {invoke} from "../../lib/bridge";
import type {ProviderDescriptor} from "../../lib/types";
export interface LaunchProfileState {
  id: string;
  install: "not_installed" | "installed" | "detected" | "unknown";
  auth: "authenticated" | "needs_auth" | "unavailable" | "unknown";
  canInstall: boolean;
  ownerConfigured: boolean;
}
export function useLaunchProfiles(provider: ProviderDescriptor | undefined) {
  const providerId = provider?.id;
  const declared = provider?.agent_runtime?.launchProfiles !== undefined;
  const [catalog, setCatalog] = useState<{providerId: string; profiles: LaunchProfileState[]} | null>(null);
  const [revision, setRevision] = useState(0);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (!providerId || !declared) return;
    let active = true;
    setError(null);
    void invoke<LaunchProfileState[]>("provider_launch_profiles", {providerId}).then(profiles => {
      if (active) setCatalog({providerId, profiles});
    }).catch(() => {if (active) {setCatalog(null); setError("Profile availability could not be checked");}});
    return () => {active = false;};
  }, [providerId, declared, revision]);
  return {profiles: catalog && catalog.providerId === providerId ? catalog.profiles : [], error, refresh: () => setRevision(value => value + 1)};
}
