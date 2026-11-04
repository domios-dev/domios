import {useState} from "react";
import {invoke} from "../../lib/bridge";
import type {ProviderDescriptor} from "../../lib/types";
import type {LaunchProfileState} from "./useLaunchProfiles";
import {Button} from "../shared/Button";

export function LaunchProfilePicker({provider, profiles, selected, onSelect, onRefresh, error}: {
  provider: ProviderDescriptor; profiles: LaunchProfileState[]; selected: string;
  onSelect: (id: string) => void; onRefresh: () => void; error: string | null;
}) {
  const [confirming, setConfirming] = useState(false);
  const [confirmed, setConfirmed] = useState(false);
  const [binary, setBinary] = useState("");
  const [args, setArgs] = useState("[]");
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const profile = profiles.find(profile => profile.id === selected);
  const declaration = provider.agent_runtime?.launchProfiles?.[selected];
  const setup = async () => {
    if (!profile || !confirmed || busy) return;
    setBusy(true); setFailure(null);
    try {
      const parsed: unknown = JSON.parse(args);
      if (!Array.isArray(parsed) || parsed.some(value => typeof value !== "string")) throw new Error("Arguments must be a JSON array of strings");
      await invoke("provider_setup_profile", {providerId: provider.id, request: {
        profileId: selected, confirmed: true,
        ...(profile.ownerConfigured ? {command: {binary, args: parsed}} : {}),
      }});
      setConfirming(false); setConfirmed(false); onRefresh();
    } catch (error) {setFailure(error instanceof Error ? error.message : "Profile setup failed");}
    finally {setBusy(false);}
  };
  return <fieldset disabled={busy} className="mt-3 flex flex-col gap-2">
    <legend>Agent profile</legend>
    <label>Profile<select aria-label="Agent profile" value={selected} onChange={event => {
      setConfirming(false); setConfirmed(false); setFailure(null); onSelect(event.target.value);
    }}>
      <option value="">Choose a profile</option>
      {Object.keys(provider.agent_runtime?.launchProfiles ?? {}).map(id => <option key={id} value={id}>{id}</option>)}
    </select></label>
    {busy && <p role="status" aria-label="Profile installation">{profile?.ownerConfigured ? "Configuring command…" : "Installing profile…"}</p>}
    {profile && !busy && <p role="status">Installation: {profile.install}. Authentication: {profile.auth}.</p>}
    {!busy && (error || failure) && <p role="alert">{failure ?? error}</p>}
    {profile && (profile.canInstall || profile.ownerConfigured) && !confirming && <Button onClick={() => setConfirming(true)}>
      {profile.ownerConfigured ? "Configure local command" : "Install profile"}
    </Button>}
    {confirming && profile && <div className="flex flex-col gap-2">
      {profile.ownerConfigured ? <>
        <label>Agent executable<input aria-label="Agent executable" value={binary} onChange={event => {setBinary(event.target.value); setConfirmed(false);}} /></label>
        <label>Arguments (JSON array)<input aria-label="Agent arguments" value={args} onChange={event => {setArgs(event.target.value); setConfirmed(false);}} /></label>
      </> : <p>Install {declaration?.install?.package} into Domios user data.</p>}
      <label><input type="checkbox" checked={confirmed} onChange={event => setConfirmed(event.target.checked)} />
        {profile.ownerConfigured ? "I trust this executable and these arguments" : "I confirm this exact package installation"}
      </label>
      <Button disabled={!confirmed || (profile.ownerConfigured && !binary.trim())} loading={busy} onClick={() => {void setup();}}>Confirm setup</Button>
      <Button onClick={() => {setConfirming(false); setConfirmed(false);}}>Cancel setup</Button>
    </div>}
  </fieldset>;
}
