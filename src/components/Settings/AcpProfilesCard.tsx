import { useEffect, useRef, useState } from "react";
import { Boxes, Loader2, Search } from "lucide-react";
import { invoke } from "../../lib/bridge";
import { invalidateAgentProviders } from "../../hooks/useAgentProviders";
import type { ProviderDescriptor } from "../../lib/types";
import { LaunchProfilePicker } from "../Terminal/LaunchProfilePicker";
import { useLaunchProfiles, type LaunchProfileState, type ProfileSetupJob } from "../Terminal/useLaunchProfiles";
import { Button } from "../shared/Button";
import { BusySpinner } from "../shared/BusyState";
import { UI_INPUT } from "../shared/design";
import { SettingsCard } from "./shared";

export interface RegistryAgent { id: string; name: string; version: string; description: string; iconUrl: string | null }

const SEARCH_DELAY_MS = 250;

/** Setup attempts that have no saved profile yet (a registry install in flight, or one that failed). */
export function registryAttempts(provider: ProviderDescriptor, profiles: readonly LaunchProfileState[]): LaunchProfileState[] {
  const declared = provider.agent_runtime?.launchProfiles ?? {};
  return profiles.filter((profile) => !(profile.id in declared) && profile.setup);
}

/**
 * The ACP agents this harness can start, with the same rows, install confirmation, progress and General agent
 * hand-off as Start a session, plus the owner's registry: search, install and remove.
 */
export function AcpProfilesCard({ provider }: { provider: ProviderDescriptor }) {
  const catalog = useLaunchProfiles(provider);
  const [selected, setSelected] = useState("");
  const attempts = registryAttempts(provider, catalog.profiles);
  const settled = useRef(new Set<string>());
  // A finished install becomes a saved profile, so the provider's declared profiles change too.
  useEffect(() => {
    for (const profile of catalog.profiles) {
      const job = profile.setup;
      if (job?.state === "installed" && !settled.current.has(job.jobId)) {
        settled.current.add(job.jobId);
        invalidateAgentProviders();
      }
    }
  }, [catalog.profiles]);
  const remove = async (id: string) => {
    await invoke("provider_remove_profile", { providerId: provider.id, name: id });
    setSelected("");
    catalog.refresh();
    invalidateAgentProviders();
  };
  return (
    <SettingsCard icon={<Boxes size={16} />} title="Agents" description="The ACP agents Domios can start. Pick one to install or set it up.">
      <div data-testid="settings-acp-profiles" className="flex flex-col gap-3">
        <LaunchProfilePicker provider={provider} profiles={catalog.profiles} selected={selected}
          onSelect={setSelected} onRefresh={catalog.refresh} error={catalog.error} onRemove={remove} localCommandForm={false} />
        {attempts.map((attempt) => <RegistryAttempt key={attempt.setup!.jobId} attempt={attempt} />)}
        <RegistrySearch providerId={provider.id} installed={new Set([...Object.keys(provider.agent_runtime?.launchProfiles ?? {}), ...attempts.map((attempt) => attempt.id)])}
          onAccepted={catalog.refresh} />
      </div>
    </SettingsCard>
  );
}

function RegistryAttempt({ attempt }: { attempt: LaunchProfileState }) {
  const job = attempt.setup!;
  if (job.state === "failed") return <p role="alert" className="m-0 text-[12px] text-[var(--ct-err)]">Could not add {attempt.id}: {job.error ?? "the install failed"}</p>;
  if (job.state === "installed") return null;
  return <p role="status" aria-label={`Adding ${attempt.id}`} className="m-0 flex items-center gap-1.5 text-[12px] text-[var(--ct-muted-fg)]">
    <BusySpinner icon={Loader2} size={12} />Adding {attempt.id}…
  </p>;
}

/** Finds agents in the ACP registry and installs one as an owner profile; progress arrives as profile setup jobs. */
function RegistrySearch({ providerId, installed, onAccepted }: { providerId: string; installed: ReadonlySet<string>; onAccepted: () => void }) {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<RegistryAgent[] | null>(null);
  const [searching, setSearching] = useState(false);
  const [installing, setInstalling] = useState<string | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  useEffect(() => {
    const term = query.trim();
    if (!term) { setResults(null); setSearching(false); return; }
    let active = true;
    setSearching(true);
    const timer = window.setTimeout(() => {
      invoke<RegistryAgent[]>("provider_registry_search", { providerId, query: term }).then((agents) => {
        if (active) { setResults(agents); setFailure(null); }
      }, (error: unknown) => {
        if (active) { setResults(null); setFailure(error instanceof Error ? error.message : String(error)); }
      }).finally(() => { if (active) setSearching(false); });
    }, SEARCH_DELAY_MS);
    return () => { active = false; window.clearTimeout(timer); };
  }, [providerId, query]);
  const install = async (agent: RegistryAgent) => {
    setInstalling(agent.id); setFailure(null);
    try {
      await invoke<ProfileSetupJob>("provider_registry_install", { providerId, id: agent.id });
      onAccepted();
      setQuery("");
    } catch (error) {
      setFailure(error instanceof Error ? error.message : String(error));
    } finally {
      setInstalling(null);
    }
  };
  return <div className="flex flex-col gap-1.5" data-testid="acp-registry">
    <div className="text-[11px] font-medium text-[var(--ct-muted-fg)]">Add from the registry</div>
    <label className="relative block">
      <Search size={13} aria-hidden className="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-[var(--ct-muted-fg)]" />
      <input type="search" aria-label="Search the ACP registry" placeholder="Search agents" value={query}
        onChange={(event) => setQuery(event.target.value)} className={`${UI_INPUT.fullCompact} !pl-8`} />
    </label>
    {searching && <p role="status" className="m-0 flex items-center gap-1.5 text-[12px] text-[var(--ct-muted-fg)]"><BusySpinner icon={Loader2} size={12} />Searching…</p>}
    {!searching && results?.length === 0 && <p className="m-0 text-[12px] text-[var(--ct-muted-fg)]">No agents match “{query.trim()}”.</p>}
    {!searching && results && results.length > 0 && <ul role="list" aria-label="Registry agents" className="m-0 flex list-none flex-col overflow-hidden rounded-lg border border-[var(--ct-border)] p-0">
      {results.map((agent) => <li key={agent.id} className="flex min-h-9 items-center gap-2.5 border-b border-[var(--ct-border)] px-3 py-1.5 text-[13px] last:border-b-0">
        <span className="min-w-0 flex-1">
          <span className="block truncate text-[var(--ct-fg)]">{agent.name} <span className="text-[11px] text-[var(--ct-muted-fg)]">{agent.version}</span></span>
          {agent.description && <span className="block truncate text-[11px] text-[var(--ct-muted-fg)]">{agent.description}</span>}
        </span>
        {installed.has(agent.id)
          ? <span className="shrink-0 text-[11px] text-[var(--ct-muted-fg)]">Added</span>
          : <Button size="compact" variant="outline" aria-label={`Install ${agent.name}`} loading={installing === agent.id} disabled={installing !== null}
            onClick={() => { void install(agent); }}>Install</Button>}
      </li>)}
    </ul>}
    {failure && <p role="alert" className="m-0 text-[12px] text-[var(--ct-err)]">{failure}</p>}
  </div>;
}
