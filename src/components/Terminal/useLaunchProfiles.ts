import {useEffect, useState} from "react";
import {invoke, listen} from "../../lib/bridge";
import {EVENT_STREAM_READY} from "../../lib/eventStreamReady";
import {onResync} from "../../lib/resync";
import type {ProviderDescriptor} from "../../lib/types";
import type {RuntimeConfigOption} from "@codeterm/plugin-sdk";
export interface LaunchProfileState {
  id: string;
  install: "not_installed" | "installed" | "detected" | "unknown";
  auth: "authenticated" | "needs_auth" | "unavailable" | "unknown";
  canInstall: boolean;
  ownerConfigured: boolean;
  setup?: ProfileSetupJob;
  /** The model and thought-level choices the agent last declared for this profile; empty until it has run once. */
  configOptions?: readonly RuntimeConfigOption[];
}
export interface ProfileSetupJob {
  jobId: string; providerId: string; profileId: string;
  state: "accepted" | "installing" | "installed" | "failed";
  error?: string;
}
export function useLaunchProfiles(provider: ProviderDescriptor | undefined) {
  const providerId = provider?.id;
  const declared = provider?.agent_runtime?.launchProfiles !== undefined;
  const [catalog, setCatalog] = useState<{providerId: string; profiles: LaunchProfileState[]} | null>(null);
  const [revision, setRevision] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [eventsError, setEventsError] = useState<string | null>(null);
  useEffect(() => {
    if (!providerId || !declared) return;
    let active = true;
    let sequence = 0;
    let unlisten: (() => void) | undefined;
    setError(null);
    setEventsError(null);
    const read = () => {
      const request = ++sequence;
      void invoke<LaunchProfileState[]>("provider_launch_profiles", {providerId}).then(profiles => {
        if (active && request === sequence) {setCatalog({providerId, profiles}); setError(null);}
      }).catch(() => {if (active && request === sequence) setError("Profile availability could not be checked");});
    };
    void listen<ProfileSetupJob>("provider-profile-setup", event => {
      if (active && event.payload.providerId === providerId) read();
    }).then(stop => {if (active) {unlisten = stop; read();} else stop();}).catch(() => {
      if (active) {setEventsError("Profile setup events are unavailable; reopen this dialog to check installation"); read();}
    });
    window.addEventListener(EVENT_STREAM_READY,read);
    const stopResync = onResync(read);
    return () => {active = false; unlisten?.(); stopResync(); window.removeEventListener(EVENT_STREAM_READY,read);};
  }, [providerId, declared, revision]);
  return {profiles: catalog && catalog.providerId === providerId ? catalog.profiles : [], error: error ?? eventsError, refresh: () => setRevision(value => value + 1)};
}
