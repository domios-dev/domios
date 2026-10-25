import { useId, useState } from "react";
import type { Tab } from "../../lib/types";
import { isProviderSelectable, providerAvailabilityLabel } from "../../lib/providerAvailability";
import { useProviderSwitch } from "./useProviderSwitch";
import { Button } from "../shared/Button";
import { DropdownSelect } from "../shared/DropdownMenu";
import { FormDialog } from "../shared/FormDialog";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { UI_INPUT } from "../shared/design";

export function ProviderSwitchDialog({ tab, initialProviderId, onClose }: { tab: Tab; initialProviderId: string; onClose(): void }) {
  const titleId = useId();
  const descriptionId = useId();
  const { providers, providerId, setProviderId, provider, selectedModel, setModel, model, setEffort, effort, efforts,
    phase, setPhase, error, result, history, ready, activity, submit, showHistory } = useProviderSwitch(tab, initialProviderId);
  const locked = phase !== "choosing";
  return <FormDialog titleId={titleId} descriptionId={descriptionId} onCancel={() => { if (phase !== "switching") onClose(); }} onSubmit={(event) => { event.preventDefault(); if (ready) void submit(); }} className="max-w-lg">
    <h2 id={titleId} className="font-semibold">Switch provider</h2>
    <p id={descriptionId} className="text-sm text-[var(--ct-muted-fg)]">This conversation continues on the new provider through a handoff. The full history is not transferred: recent messages and task context are sent, and the target provider builds a working summary. The tab, role and task stay the same.</p>
    {phase !== "complete" && <>
      <DropdownSelect aria-label="Target provider" value={providerId} options={providers.map((provider) => ({ value: provider.id, label: isProviderSelectable(provider) ? provider.display_name : `${provider.display_name} (${providerAvailabilityLabel(provider)})`, disabled: !isProviderSelectable(provider) }))} disabled={locked} onChange={(id) => { setProviderId(id); setModel(undefined); setEffort(undefined); }} />
      {(provider?.available_models.length ?? 0) > 0 ? <DropdownSelect aria-label="Target model" value={model ?? ""} disabled={locked} options={(provider?.available_models ?? []).map((model) => ({ value: model.id, label: model.display_name, disabled: model.available === false }))} onChange={(model) => { setModel(model); setEffort(undefined); }} /> : <label className="text-sm">Model (optional)<input className={UI_INPUT.full} value={selectedModel ?? ""} disabled={locked} onChange={(event) => setModel(event.target.value || undefined)} /></label>}
      {efforts.length > 0 && <DropdownSelect aria-label="Reasoning effort" value={effort ?? ""} options={efforts.map((effort) => ({ value: effort.id, label: effort.display_name }))} disabled={locked} onChange={setEffort} />}
    </>}
    {error && <InlineErrorNotice error={error} operation="switching the provider" actions={error.includes("provider_switch_auth_timeout") ? [{ label: "Retry", onClick: () => void submit(), disabled: !ready || !provider }] : []} />}
    {phase === "switching" && <p role="status">Retiring the previous session, binding the target and delivering the handoff…</p>}
    {phase === "waiting" && <p role="status">Waiting for this tab to become idle. Cancel to keep the current provider.</p>}
    {result && <div role="status" className="text-sm">Continued on {result.provider}. Handoff delivered and task binding preserved.</div>}
    {result && <Button size="sm" onClick={() => void showHistory()}>Previous provider: {result.previous_provider}</Button>}
    {history && <section aria-label="Previous provider transcript" className="max-h-72 overflow-auto text-sm">{history.map((row, index) => <article key={index}><strong>{row.kind}</strong><pre className="whitespace-pre-wrap">{row.content}</pre></article>)}</section>}
    <div className="flex justify-end gap-2">
      <Button size="sm" disabled={phase === "switching"} onClick={onClose}>{phase === "complete" ? "Close" : "Cancel"}</Button>
      {phase === "choosing" && !ready && <>
        <Button size="sm" onClick={() => setPhase("waiting")} disabled={!provider}>Wait for idle</Button>
        <Button size="sm" variant="danger" onClick={() => void submit(true)} disabled={!provider || activity.launching}>Stop and switch</Button>
      </>}
      {phase === "choosing" && ready && <Button size="sm" variant="primary" type="submit" disabled={!provider}>Switch provider</Button>}
    </div>
  </FormDialog>;
}

export function useProviderSwitchDialog(tab: Tab | null) {
  const [providerId, openProviderSwitch] = useState<string | null>(null);
  return {
    openProviderSwitch,
    providerSwitchDialog: tab && providerId ? <ProviderSwitchDialog tab={tab} initialProviderId={providerId} onClose={() => openProviderSwitch(null)} /> : null,
  };
}
