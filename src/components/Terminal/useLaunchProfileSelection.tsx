import {useState} from "react";
import type {ProviderDescriptor} from "../../lib/types";
import {LaunchProfileModelRow, LaunchProfilePicker, LaunchProfileSelect} from "./LaunchProfilePicker";
import {useLaunchProfiles} from "./useLaunchProfiles";

export function useLaunchProfileSelection(provider: ProviderDescriptor | undefined, testId: string) {
  const catalog = useLaunchProfiles(provider);
  const [selection, setSelection] = useState({providerId: "", id: ""});
  const profile = selection.providerId === provider?.id ? selection.id : "";
  const [choice, setChoice] = useState({key: "", model: "", effort: ""});
  const key = `${provider?.id}:${profile}`;
  const {model, effort} = choice.key === key ? choice : {model: "", effort: ""};
  const enabled = !!provider?.agent_runtime?.launchProfiles;
  const readyProfile = enabled ? catalog.profiles.find(entry => entry.id === profile
    && (entry.install === "installed" || entry.install === "detected")) : undefined;
  const onSelect = (id: string) => setSelection({providerId: provider?.id ?? "", id});

  return {
    profile: enabled ? profile : "",
    model,
    effort,
    ready: !enabled || !!readyProfile,
    modelSlot: enabled && provider ? <LaunchProfileSelect provider={provider} profiles={catalog.profiles}
      selected={profile} testId={testId} onSelect={onSelect} /> : undefined,
    details: enabled && provider ? <>
      {readyProfile && <LaunchProfileModelRow profile={readyProfile} model={model} effort={effort}
        onModel={model => setChoice({key, model, effort})}
        onEffort={effort => setChoice({key, model, effort})} />}
      <LaunchProfilePicker key={key} list={false} provider={provider} profiles={catalog.profiles}
        selected={profile} onSelect={onSelect} onRefresh={catalog.refresh} error={catalog.error} />
    </> : undefined,
  };
}
