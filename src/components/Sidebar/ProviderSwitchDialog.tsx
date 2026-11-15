import { useId, useState } from "react";
import type { Tab } from "../../lib/types";
import { isProviderSelectable, providerAvailabilityLabel } from "../../lib/providerAvailability";
import { useProviderSwitch } from "./useProviderSwitch";
import { Button } from "../shared/Button";
import { DropdownSelect } from "../shared/DropdownMenu";
import { FormDialog } from "../shared/FormDialog";
import { describeError } from "../../lib/describeError";
import { UI_INPUT } from "../shared/design";

export function ProviderSwitchDialog({ tab, initialProviderId, onClose }: { tab: Tab; initialProviderId: string; onClose(): void }) {
  const titleId = useId();
  const descriptionId = useId();
  const { providers, providerId, setProviderId, provider, selectedModel, setModel, model, setEffort, effort, efforts,
    phase, setPhase, error, retryAfterSeconds, result, history, ready, activity, submit, showHistory } = useProviderSwitch(tab, initialProviderId);
  const locked = phase !== "choosing";
  return <FormDialog titleId={titleId} descriptionId={descriptionId} onCancel={() => { if (phase !== "switching") onClose(); }} onSubmit={(event) => { event.preventDefault(); if (ready) void submit(); }} className="max-w-lg">
    <h2 id={titleId} className="font-semibold">Switch harness</h2>
    <p id={descriptionId} className="text-sm text-[var(--ct-muted-fg)]">Continue on another harness using recent messages and task context. The full history stays here.</p>
    {phase !== "complete" && <>
      <DropdownSelect aria-label="Target provider" value={providerId} options={providers.map((provider) => ({ value: provider.id, label: isProviderSelectable(provider) ? provider.display_name : `${provider.display_name} (${providerAvailabilityLabel(provider)})`, disabled: !isProviderSelectable(provider) }))} disabled={locked} onChange={(id) => { setProviderId(id); setModel(undefined); setEffort(undefined); }} />
      {(provider?.available_models.length ?? 0) > 0 ? <DropdownSelect aria-label="Target model" value={model ?? ""} disabled={locked} options={(provider?.available_models ?? []).map((model) => ({ value: model.id, label: model.display_name, disabled: model.available === false }))} onChange={(model) => { setModel(model); setEffort(undefined); }} /> : <label className="text-sm">Model (optional)<input className={UI_INPUT.full} value={selectedModel ?? ""} disabled={locked} onChange={(event) => setModel(event.target.value || undefined)} /></label>}
      {efforts.length > 0 && <DropdownSelect aria-label="Reasoning effort" value={effort ?? ""} options={efforts.map((effort) => ({ value: effort.id, label: effort.display_name }))} disabled={locked} onChange={setEffort} />}
    </>}
    {error && <SwitchFailure error={error} />}
    {phase === "switching" && <p role="status">Switching harness…</p>}
    {phase === "waiting" && <p role="status">Waiting for this tab to become idle. Cancel to keep the current harness.</p>}
    {result && <div role="status" className="text-sm">Continued on {result.provider}.</div>}
    {result && <Button size="sm" onClick={() => void showHistory()}>Previous harness: {result.previous_provider}</Button>}
    {history && <section aria-label="Previous harness transcript" className="max-h-72 overflow-auto text-sm">{history.map((row, index) => <article key={index}><strong>{row.kind}</strong><pre className="whitespace-pre-wrap">{row.content}</pre></article>)}</section>}
    <div className="flex justify-end gap-2">
      <Button size="sm" disabled={phase === "switching"} onClick={onClose}>{phase === "complete" ? "Close" : "Cancel"}</Button>
      {phase === "choosing" && !ready && <>
        <Button size="sm" onClick={() => setPhase("waiting")} disabled={!provider || retryAfterSeconds !== null}>Wait for idle</Button>
        <Button size="sm" variant="danger" onClick={() => void submit(true)} disabled={!provider || activity.launching || retryAfterSeconds !== null}>Stop and switch</Button>
      </>}
      {phase === "choosing" && ready && <Button size="sm" variant="primary" type="submit" disabled={!provider || retryAfterSeconds !== null}>{retryAfterSeconds !== null ? `Switch in ${retryAfterSeconds} s` : "Switch harness"}</Button>}
    </div>
  </FormDialog>;
}

function SwitchFailure({ error }: { error: string }) {
  const view = describeError(error, { operation: "switching the harness" });
  const [expanded, setExpanded] = useState(false);
  const [copyState, setCopyState] = useState("");
  const detailsId = useId();
  async function copy() {
    try { await navigator.clipboard.writeText(view.raw); setCopyState("Copied"); }
    catch { setCopyState("Select the details to copy them."); }
  }
  return <section role="status" aria-label={view.title} className="space-y-2 text-sm">
    <p>{view.title}; {view.cause ? view.cause[0].toLowerCase() + view.cause.slice(1) : "try again."}</p>
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
