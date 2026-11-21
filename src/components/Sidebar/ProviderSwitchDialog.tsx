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
    phase, setPhase, error, statusUncertainty, retryAfterSeconds, switchStatus, submittedTarget, ready, activity, submit, stopObserving } = useProviderSwitch(tab, initialProviderId);
  const navigation = usePickerNavigation();
  const [installError, setInstallError] = useState<string | null>(null);
  const locked = phase !== "choosing";
  // Same order as Start a session: launch-profile providers follow the direct ones.
  const ordered = [...providers.filter((candidate) => !candidate.agent_runtime?.launchProfiles), ...providers.filter((candidate) => candidate.agent_runtime?.launchProfiles)];
  const unavailableIds = useMemo(() => new Set(providers.filter((candidate) => !isProviderSelectable(candidate)).map((candidate) => candidate.id)), [providers]);
  const switchingTarget = phase === "switching" ? submittedTarget : null;
  const visibleProvider = switchingTarget?.provider ?? provider;
  const freeModel = visibleProvider && visibleProvider.available_models.length === 0
    ? <input aria-label="Agent model" placeholder="Model (optional)" className={SELECT_TRIGGER} value={(switchingTarget ? switchingTarget.model : selectedModel) ?? ""} disabled={locked}
      onChange={(event) => setModel(event.target.value || undefined)} />
    : undefined;
  function closeDialog() {
    stopObserving();
    onClose();
  }
  async function install(id: string) {
    setInstallError(null);
    try {
      const launch = await launchProviderInstaller(id);
      closeDialog();
      const workspaceId = launch.workspace_id ?? tab.workspace_id;
      if (workspaceId) await navigation.show({ workspaceId, tabId: launch.tab_id });
    } catch (failure) { setInstallError(failure instanceof Error ? failure.message : String(failure)); }
  }
  const tabName = tab.title || "This tab";
  const target = phase === "choosing" || phase === "waiting" ? null : submittedTarget;
  const targetName = target?.provider.display_name ?? visibleProvider?.display_name ?? "the new provider";
  const modelName = target?.provider.available_models.find((entry) => entry.id === target.model)?.display_name;
  const statusView = providerSwitchStatusView(switchStatus, targetName, tabName, modelName);
  const failure = phase === "complete" ? null : statusView?.kind === "failed" ? statusView.reason : error ?? installError;
  const closeLabel = phase === "switching" || phase === "complete" ? "Close" : "Cancel";
  return <FormDialog titleId={titleId} descriptionId={descriptionId} onCancel={closeDialog} onSubmit={(event) => { event.preventDefault(); if (phase === "choosing" && ready) void submit(); }} className="max-w-lg">
    <h2 id={titleId} className={phase === "complete" ? "sr-only" : "font-semibold"}>Switch provider</h2>
    {phase !== "complete" && <p id={descriptionId} className="text-sm text-[var(--ct-muted-fg)]">
      {tabName} continues with another provider, using recent messages and task context. The full history stays here.
      {statusView?.kind === "busy" && " You can close this dialog while the switch continues."}
    </p>}
    {phase !== "complete" && <OrchestrationRoleRow
      label="Agent"
      providers={switchingTarget ? [switchingTarget.provider] : ordered}
      providerId={providerId}
      modelId={(switchingTarget ? switchingTarget.model : model) ?? ""}
      effortId={switchingTarget ? switchingTarget.effort : effort}
      disabledProviderIds={switchingTarget ? undefined : unavailableIds}
      disabled={locked}
      showCurrentModel={false}
      showEffortLabel
      providerDescribedBy={failure ? errorId : undefined}
      modelSlot={freeModel}
      onProviderChange={(id) => { setInstallError(null); setProviderId(id); setModel(undefined); setEffort(undefined); }}
      onModelChange={(next) => { setModel(next); setEffort(undefined); }}
      onEffortChange={setEffort}
      onInstallProvider={(id) => void install(id)}
    />}
    {failure && <SwitchFailure error={failure} id={errorId} providerName={provider?.display_name ?? submittedTarget?.provider.display_name} operation={error || statusView?.kind === "failed" ? "switching providers" : "starting the installer"} />}
    {statusView?.kind === "busy" && <div role="status" aria-live="polite" className="flex items-center gap-2 text-sm text-[var(--ct-muted-fg)]">
      <span aria-hidden="true"><BusySpinner icon={LoaderCircle} size={16} /></span>
      <span>{statusView.label}</span>
    </div>}
    {statusView?.kind === "complete" && <p id={descriptionId} role="status" className="text-sm">{statusView.label}</p>}
    {statusUncertainty && <div role="alert" aria-live="assertive" className="text-sm">
      <p>The switch may have completed, but Domios could not confirm its status. Check this tab before trying again.</p>
      <p className="mt-1 text-[var(--ct-muted-fg)]">{statusUncertainty}</p>
    </div>}
    {phase === "waiting" && <p role="status">Waiting for this tab to become idle. Cancel to keep the current provider.</p>}
    <div className="flex justify-end gap-2">
      <Button size="sm" onClick={closeDialog}>{closeLabel}</Button>
      {phase === "choosing" && !ready && <>
        <Button size="sm" onClick={() => setPhase("waiting")} disabled={!provider || retryAfterSeconds !== null}>Wait for idle</Button>
        <Button size="sm" variant="danger" onClick={() => void submit(true)} disabled={!provider || activity.launching || retryAfterSeconds !== null}>Stop and switch</Button>
      </>}
      {phase === "choosing" && ready && <Button size="sm" variant="primary" type="submit" disabled={!provider || retryAfterSeconds !== null}>{retryAfterSeconds !== null ? `Switch in ${retryAfterSeconds} s` : "Switch"}</Button>}
    </div>
  </FormDialog>;
}

function SwitchFailure({ error, id, providerName, operation }: { error: string; id: string; providerName?: string; operation: string }) {
  const view = describeError(error, { operation, providerName });
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
