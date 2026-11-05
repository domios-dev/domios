import {useState} from "react";
import {invoke} from "../../lib/bridge";
import type {ProviderDescriptor} from "../../lib/types";
import type {LaunchProfileState, ProfileSetupJob} from "./useLaunchProfiles";
import {AgentIcon} from "../AgentIcon";
import {Button} from "../shared/Button";

export function LaunchProfilePicker({provider, profiles, selected, onSelect, onRefresh, error}: {
  provider: ProviderDescriptor; profiles: LaunchProfileState[]; selected: string;
  onSelect: (id: string) => void; onRefresh: () => void; error: string | null;
}) {
  const [confirming, setConfirming] = useState(false);
  const [confirmed, setConfirmed] = useState(false);
  const [binary, setBinary] = useState("");
  const [args, setArgs] = useState("[]");
  const [submitting, setSubmitting] = useState(false);
  const [accepted, setAccepted] = useState<{job: ProfileSetupJob; profile: LaunchProfileState} | null>(null);
  const [attemptBaseJobId, setAttemptBaseJobId] = useState<string | undefined>();
  const [failure, setFailure] = useState<string | null>(null);
  const profile = profiles.find(profile => profile.id === selected);
  const declaration = provider.agent_runtime?.launchProfiles?.[selected];
  const currentAcceptance = accepted?.job.providerId === provider.id && accepted.job.profileId === selected && accepted.profile === profile ? accepted.job : null;
  const job = !profile ? undefined : currentAcceptance && profile.setup?.jobId !== currentAcceptance.jobId ? currentAcceptance : profile.setup;
  const busy = submitting || job?.state === "accepted" || job?.state === "installing";
  const setupFailure = job?.state === "failed" ? job.error ?? "Profile setup failed"
    : job?.state === "installed" && job.jobId !== attemptBaseJobId ? null : failure;
  const localCommand = profile?.ownerConfigured && !profile.canInstall;
  const setup = async () => {
    if (!profile || !confirmed || busy) return;
    setSubmitting(true); setFailure(null); setAttemptBaseJobId(profile.setup?.jobId); setAccepted(null);
    try {
      const parsed: unknown = JSON.parse(args);
      if (!Array.isArray(parsed) || parsed.some(value => typeof value !== "string")) throw new Error("Arguments must be a JSON array of strings");
      const job = await invoke<ProfileSetupJob>("provider_setup_profile", {providerId: provider.id, request: {
        profileId: selected, confirmed: true,
        ...(localCommand ? {command: {binary, args: parsed}} : {}),
      }});
      setAccepted({job, profile});
      setConfirming(false); setConfirmed(false); onRefresh();
    } catch (error) {setFailure(error instanceof Error ? error.message : "Profile setup failed");}
    finally {setSubmitting(false);}
  };
  return <fieldset disabled={busy} className="mt-3 flex flex-col gap-2">
    <legend>Agent profile</legend>
    <AgentIcon providerId={provider.id} profileId={selected} title={selected || provider.display_name} />
    <label>Profile<select aria-label="Agent profile" value={selected} onChange={event => {
      setConfirming(false); setConfirmed(false); setFailure(null); setAccepted(null); onSelect(event.target.value);
    }}>
      <option value="">Choose a profile</option>
      {Object.keys(provider.agent_runtime?.launchProfiles ?? {}).map(id => <option key={id} value={id}>{id}</option>)}
    </select></label>
    {busy && <p role="status" aria-label="Profile installation">{localCommand ? "Configuring command…" : "Installing profile…"}</p>}
    {busy && error && <p role="status">{error}</p>}
    {profile && !busy && <p role="status">Installation: {profile.install}. Authentication: {profile.auth}.</p>}
    {!busy && (error || setupFailure) && <p role="alert">{setupFailure ?? error}</p>}
    {profile && (profile.canInstall || profile.ownerConfigured) && !confirming && <Button onClick={() => setConfirming(true)}>
      {localCommand ? "Configure local command" : "Install profile"}
    </Button>}
    {confirming && profile && <div className="flex flex-col gap-2">
      {localCommand ? <>
        <label>Agent executable<input aria-label="Agent executable" value={binary} onChange={event => {setBinary(event.target.value); setConfirmed(false);}} /></label>
        <label>Arguments (JSON array)<input aria-label="Agent arguments" value={args} onChange={event => {setArgs(event.target.value); setConfirmed(false);}} /></label>
      </> : <p>Install {declaration?.install?.package} into Domios user data.</p>}
      <label><input type="checkbox" checked={confirmed} onChange={event => setConfirmed(event.target.checked)} />
        {localCommand ? "I trust this executable and these arguments" : "I confirm this exact package installation"}
      </label>
      <Button disabled={!confirmed || (localCommand && !binary.trim())} loading={busy} onClick={() => {void setup();}}>Confirm setup</Button>
      <Button onClick={() => {setConfirming(false); setConfirmed(false);}}>Cancel setup</Button>
    </div>}
  </fieldset>;
}
