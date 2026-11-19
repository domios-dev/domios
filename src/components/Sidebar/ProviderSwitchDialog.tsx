import { useId, useMemo, useState } from "react";
import { LoaderCircle } from "lucide-react";
import type { Tab } from "../../lib/types";
import { isProviderSelectable } from "../../lib/providerAvailability";
import { launchProviderInstaller } from "../../lib/providerInstall";
import { useProviderSwitch } from "./useProviderSwitch";
import { OrchestrationRoleRow } from "./OrchestrationPairForm";
import { usePickerNavigation } from "../AgentPicker/pickerNavigation";
import { Button } from "../shared/Button";
import { SELECT_TRIGGER } from "../shared/DropdownMenu";
import { FormDialog } from "../shared/FormDialog";
import { BusySpinner } from "../shared/BusyState";
import { describeError } from "../../lib/describeError";
import { providerSwitchStatusView } from "./providerSwitchStatusModel";

export function ProviderSwitchDialog({ tab, initialProviderId, onClose }: { tab: Tab; initialProviderId: string; onClose(): void }) {
  const titleId = useId();
  const descriptionId = useId();
  const errorId = useId();
  const { providers, providerId, setProviderId, provider, selectedModel, setModel, model, setEffort, effort,
    phase, setPhase, error, statusUncertainty, observationLost, retryAfterSeconds, switchStatus, result, history, ready, activity, submit, showHistory } = useProviderSwitch(tab, initialProviderId);
  const navigation = usePickerNavigation();
  const [installError, setInstallError] = useState<string | null>(null);
  const locked = phase !== "choosing";
  // Same order as Start a session: launch-profile harnesses follow the direct ones.
  const ordered = [...providers.filter((candidate) => !candidate.agent_runtime?.launchProfiles), ...providers.filter((candidate) => candidate.agent_runtime?.launchProfiles)];
  const unavailableIds = useMemo(() => new Set(providers.filter((candidate) => !isProviderSelectable(candidate)).map((candidate) => candidate.id)), [providers]);
  const freeModel = provider && provider.available_models.length === 0
    ? <input aria-label="Agent model" placeholder="Model (optional)" className={SELECT_TRIGGER} value={selectedModel ?? ""} disabled={locked}
      onChange={(event) => setModel(event.target.value || undefined)} />
    : undefined;
  async function install(id: string) {
    setInstallError(null);
    try {
      const launch = await launchProviderInstaller(id);
      onClose();
      const workspaceId = launch.workspace_id ?? tab.workspace_id;
      if (workspaceId) await navigation.show({ workspaceId, tabId: launch.tab_id });
    } catch (failure) { setInstallError(failure instanceof Error ? failure.message : String(failure)); }
  }
  const statusView = providerSwitchStatusView(switchStatus, provider?.display_name ?? "the new provider");
  const failure = statusView?.kind === "failed" ? statusView.reason : error ?? installError;
  const showObservationLost = observationLost && phase === "switching";
  const canExit = phase !== "switching" || showObservationLost || statusUncertainty !== null;
  const closeLabel = phase === "complete" || showObservationLost || statusUncertainty !== null ? "Close" : "Cancel";
  return <FormDialog titleId={titleId} descriptionId={descriptionId} onCancel={() => { if (canExit) onClose(); }} onSubmit={(event) => { event.preventDefault(); if (ready) void submit(); }} className="max-w-lg">
    <h2 id={titleId} className="font-semibold">Switch harness</h2>
    <p id={descriptionId} className="text-sm text-[var(--ct-muted-fg)]">{tab.title || "This tab"} continues on another harness using recent messages and task context. The full history stays here.</p>
    {phase !== "complete" && <OrchestrationRoleRow
      label="Agent"
      providers={ordered}
      providerId={providerId}
      modelId={model ?? ""}
      effortId={effort}
      disabledProviderIds={unavailableIds}
      disabled={locked}
      providerDescribedBy={failure ? errorId : undefined}
      modelSlot={freeModel}
      onProviderChange={(id) => { setInstallError(null); setProviderId(id); setModel(undefined); setEffort(undefined); }}
      onModelChange={(next) => { setModel(next); setEffort(undefined); }}
      onEffortChange={setEffort}
      onInstallProvider={(id) => void install(id)}
    />}
    {failure && <SwitchFailure error={failure} id={errorId} harnessName={provider?.display_name} operation={error || statusView?.kind === "failed" ? "switching the harness" : "starting the installer"} />}
    {statusView?.kind === "busy" && <div role="status" aria-live="polite" className="flex items-center gap-3 rounded-md border border-[var(--ct-border)] p-3 text-sm">
      <span aria-hidden="true"><BusySpinner icon={LoaderCircle} size={16} /></span>
      <span>{statusView.label}</span>
    </div>}
    {statusView?.kind === "complete" && <div role="status" aria-live="polite" className="text-sm">{statusView.label}</div>}
    {(showObservationLost || statusUncertainty) && <div role="alert" aria-live="assertive" className="text-sm">
      {showObservationLost
        ? <p>Live status updates are disconnected. Closing this dialog will not cancel the provider switch.</p>
        : <p>The switch may have completed, but Domios could not confirm its status. Check this tab before trying again.</p>}
      {statusUncertainty && <p className="mt-1 text-[var(--ct-muted-fg)]">{statusUncertainty}</p>}
    </div>}
    {phase === "waiting" && <p role="status">Waiting for this tab to become idle. Cancel to keep the current harness.</p>}
    {result && <Button size="sm" onClick={() => void showHistory()}>Previous harness: {result.previous_provider}</Button>}
    {history && <section aria-label="Previous harness transcript" className="max-h-72 overflow-auto text-sm">{history.map((row, index) => <article key={index}><strong>{row.kind}</strong><pre className="whitespace-pre-wrap">{row.content}</pre></article>)}</section>}
    <div className="flex justify-end gap-2">
      <Button size="sm" disabled={!canExit} onClick={onClose}>{closeLabel}</Button>
      {phase === "choosing" && !ready && <>
        <Button size="sm" onClick={() => setPhase("waiting")} disabled={!provider || retryAfterSeconds !== null}>Wait for idle</Button>
        <Button size="sm" variant="danger" onClick={() => void submit(true)} disabled={!provider || activity.launching || retryAfterSeconds !== null}>Stop and switch</Button>
      </>}
      {phase === "choosing" && ready && <Button size="sm" variant="primary" type="submit" disabled={!provider || retryAfterSeconds !== null}>{retryAfterSeconds !== null ? `Switch in ${retryAfterSeconds} s` : "Switch harness"}</Button>}
    </div>
  </FormDialog>;
}

function SwitchFailure({ error, id, harnessName, operation }: { error: string; id: string; harnessName?: string; operation: string }) {
  const view = describeError(error, { operation, harnessName });
  const [expanded, setExpanded] = useState(false);
  const [copyState, setCopyState] = useState("");
  const detailsId = useId();
  async function copy() {
    try { await navigator.clipboard.writeText(view.raw); setCopyState("Copied"); }
    catch { setCopyState("Select the details to copy them."); }
  }
  return <section role="status" aria-live="polite" aria-label={view.title} className="space-y-2 text-sm">
    <p id={id}>{view.title}; {view.cause ? view.cause[0].toLowerCase() + view.cause.slice(1) : "try again."}</p>
    <div className="flex items-center gap-3 text-xs text-[var(--ct-muted-fg)]">
      <button type="button" aria-expanded={expanded} aria-controls={detailsId} onClick={() => setExpanded(!expanded)} className="hover:text-[var(--ct-fg)]">{expanded ? "Hide details" : "Details"}</button>
      <button type="button" onClick={() => void copy()} className="hover:text-[var(--ct-fg)]">{copyState || "Copy"}</button>
    </div>
    <pre id={detailsId} hidden={!expanded} tabIndex={0} className="max-h-40 select-text overflow-auto whitespace-pre-wrap break-words text-xs text-[var(--ct-muted-fg)] [overflow-wrap:anywhere]">{view.raw}</pre>
  </section>;
}

export function useProviderSwitchDialog(tab: Tab | null) {
  const [providerId, openProviderSwitch] = useState<string | null>(null);
  return {
    openProviderSwitch,
    providerSwitchDialog: tab && providerId ? <ProviderSwitchDialog tab={tab} initialProviderId={providerId} onClose={() => openProviderSwitch(null)} /> : null,
  };
}
