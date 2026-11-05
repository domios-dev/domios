import {useState} from "react";
import {Check, Loader2} from "lucide-react";
import {invoke} from "../../lib/bridge";
import {startAcpSetup} from "../../lib/acpSetup";
import {useAiHelperLaunch} from "../../lib/useAiHelperLaunch";
import type {ProviderDescriptor} from "../../lib/types";
import type {LaunchProfileState, ProfileSetupJob} from "./useLaunchProfiles";
import {AgentIcon} from "../AgentIcon";
import {Button} from "../shared/Button";
import {BusySpinner} from "../shared/BusyState";
import {UI_INPUT} from "../shared/design";

/** A profile's state in a word or two; the row stays quiet and the detail lives in its tooltip. */
export function profileStateLabel(profile: LaunchProfileState | undefined, job: ProfileSetupJob | undefined): string {
  if (job?.state === "accepted" || job?.state === "installing") return "Installing…";
  if (job?.state === "failed") return "Setup failed";
  if (!profile) return "";
  if (profile.install === "installed" || profile.install === "detected") {
    return profile.auth === "needs_auth" ? "Sign in needed" : profile.auth === "authenticated" ? "Ready" : "Installed";
  }
  if (profile.install === "not_installed") return profile.canInstall ? "Not installed" : profile.ownerConfigured ? "Not set up" : "Unavailable";
  return "";
}

const ready = (profile: LaunchProfileState) => profile.install === "installed" || profile.install === "detected";

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
  const helper = useAiHelperLaunch("acp-setup-failure");
  const ids = Object.keys(provider.agent_runtime?.launchProfiles ?? {});
  const profile = profiles.find(profile => profile.id === selected);
  const declaration = provider.agent_runtime?.launchProfiles?.[selected];
  const currentAcceptance = accepted?.job.providerId === provider.id && accepted.job.profileId === selected && accepted.profile === profile ? accepted.job : null;
  const job = !profile ? undefined : currentAcceptance && profile.setup?.jobId !== currentAcceptance.jobId ? currentAcceptance : profile.setup;
  const busy = submitting || job?.state === "accepted" || job?.state === "installing";
  const setupFailure = job?.state === "failed" ? job.error ?? "Profile setup failed"
    : job?.state === "installed" && job.jobId !== attemptBaseJobId ? null : failure;
  const localCommand = profile?.ownerConfigured && !profile.canInstall;
  const noneReady = profiles.length > 0 ? !profiles.some(ready) : ids.length === 0;
  const choose = (id: string) => {
    if (busy) return;
    setConfirming(false); setConfirmed(false); setFailure(null); setAccepted(null); onSelect(id);
  };
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
  return <div className="mt-3 flex flex-col gap-1.5" data-testid="launch-profile-picker">
    <div id="launch-profile-label" className="text-[11px] font-medium text-[var(--ct-muted-fg)]">Agent profile</div>
    {ids.length > 0 && <div role="radiogroup" aria-labelledby="launch-profile-label" aria-label="Agent profile" aria-disabled={busy || undefined}
      className="flex flex-col overflow-hidden rounded-lg border border-[var(--ct-border)]">
      {ids.map(id => {
        const state = profiles.find(entry => entry.id === id);
        const current = id === selected;
        const label = profileStateLabel(state, current ? job : state?.setup);
        return <button key={id} type="button" role="radio" aria-checked={current} disabled={busy && !current} onClick={() => choose(id)}
          title={state ? `Installation: ${state.install}. Authentication: ${state.auth}.` : undefined}
          className={`flex min-h-9 w-full items-center gap-2.5 border-b border-[var(--ct-border)] px-3 py-1.5 text-left text-[13px] last:border-b-0 ${current ? "bg-[color-mix(in_srgb,var(--ct-accent)_10%,transparent)]" : "hover:bg-[var(--ct-hover)]"}`}>
          <AgentIcon providerId={provider.id} profileId={id} title={id} className="shrink-0 text-[14px]" />
          <span className="min-w-0 flex-1 truncate text-[var(--ct-fg)]">{id}</span>
          {label && <span className="shrink-0 text-[11px] text-[var(--ct-muted-fg)]">{label}</span>}
          <span className="flex w-3.5 shrink-0 justify-center">{current && <Check size={13} strokeWidth={2.5} aria-hidden className="text-[var(--ct-accent)]" />}</span>
        </button>;
      })}
    </div>}
    {busy && <p role="status" aria-label="Profile installation" className="m-0 flex items-center gap-1.5 text-[12px] text-[var(--ct-muted-fg)]">
      <BusySpinner icon={Loader2} size={12} />{localCommand ? "Configuring command…" : `Installing ${selected}…`}
    </p>}
    {busy && error && <p role="status" className="m-0 text-[12px] text-[var(--ct-muted-fg)]">{error}</p>}
    {!busy && (error || setupFailure) && <p role="alert" className="m-0 text-[12px] text-[var(--ct-err)]">{setupFailure ?? error}</p>}
    {confirming && profile && <div className="flex flex-col gap-2 rounded-lg border border-[var(--ct-border)] p-2.5 text-[12px]">
      {localCommand ? <>
        <input aria-label="Agent executable" placeholder="Agent executable" className={UI_INPUT.fullCompact} value={binary} onChange={event => {setBinary(event.target.value); setConfirmed(false);}} />
        <input aria-label="Agent arguments" placeholder='Arguments, as a JSON array: ["--acp"]' className={UI_INPUT.fullMono} value={args} onChange={event => {setArgs(event.target.value); setConfirmed(false);}} />
      </> : <p className="m-0 text-[var(--ct-muted-fg)]">Installs <span className="ct-text-mono text-[var(--ct-fg)]">{declaration?.install?.package}</span> into Domios user data.</p>}
      <label className="flex items-center gap-2 text-[var(--ct-fg)]"><input type="checkbox" checked={confirmed} onChange={event => setConfirmed(event.target.checked)} />
        {localCommand ? "I trust this executable and these arguments" : "I confirm this exact package installation"}
      </label>
      <div className="flex justify-end gap-2">
        <Button size="compact" variant="ghost" onClick={() => {setConfirming(false); setConfirmed(false);}}>Cancel setup</Button>
        <Button size="compact" variant="primary" disabled={!confirmed || (localCommand && !binary.trim())} loading={busy} onClick={() => {void setup();}}>Confirm setup</Button>
      </div>
    </div>}
    {noneReady && <p className="m-0 text-[12px] text-[var(--ct-muted-fg)]">No ACP agents set up yet. ACP connects almost any coding agent.</p>}
    <div className="flex min-h-7 flex-wrap items-center justify-between gap-2 text-[12px]">
      {profile && !busy && (profile.canInstall || profile.ownerConfigured) && !confirming
        ? <Button size="compact" variant={ready(profile) ? "ghost" : "outline"} onClick={() => setConfirming(true)}>{localCommand ? "Configure local command" : "Install profile"}</Button>
        : <span />}
      <button type="button" className="text-[var(--ct-accent)] hover:underline disabled:opacity-60" disabled={helper.launching}
        onClick={() => helper.launch(() => startAcpSetup(noneReady ? undefined : selected || undefined))}>
        {helper.launching ? "Opening…" : noneReady ? "Ask the General agent" : "Set up with the General agent"}
      </button>
    </div>
  </div>;
}
