import { useState } from "react";
import { Button } from "../shared/Button";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import type { NeighborRuntimeClient, ReceiverState } from "./neighborRuntimeClient";
import { grantSummary, useReceiverGrants, type ReceiverGrantDecision } from "./neighborReceiverGrants";
import { NeighborRuntimeFailure } from "./neighborRuntimeClient";
import MyMachines from "../MeshPanel/AddPeerPanel";
import { receiverFailureDescription } from "./neighborReceiverError";

function takeoverWarning(receiver: ReceiverState, targetMachine: string, friendName: string, operation: "set" | "reset"): string {
  if (operation === "reset") return `Stop receiving ${friendName}'s messages on this machine. Incoming messages wait until a concierge machine is chosen.`;
  if (receiver.receiving_enabled && !receiver.receiving_here) return `${friendName} is received on ${receiver.receiver_machine_label ?? "another machine"}. Move here? Their concierge starts fresh with the permissions chosen below.`;
  return `Receive ${friendName}'s messages on ${targetMachine}. Their concierge uses this machine's permissions.`;
}

export function NeighborReceiveHere({ receiver, onReceive, friendship, client, friendName, targetMachine = "This machine", operation = "set", initiallyOpen = false, onClose }: {
  friendship: string;
  client: Pick<NeighborRuntimeClient, "receiverGrants">;
  receiver: ReceiverState;
  onReceive: (revision: number, decision?: ReceiverGrantDecision) => Promise<void>;
  friendName: string;
  targetMachine?: string;
  operation?: "set" | "reset";
  initiallyOpen?: boolean;
  onClose?: () => void;
}) {
  const binding = JSON.stringify([receiver.revision, receiver.receiver_machine_label, receiver.receiving_here, receiver.receiving_enabled, operation, targetMachine]);
  const [confirming, setConfirming] = useState<string | null>(initiallyOpen ? binding : null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<ReturnType<typeof receiverFailureDescription> | null>(null);
  const [keyLimited, setKeyLimited] = useState(false);
  const [managing, setManaging] = useState(false);
  const grants = useReceiverGrants(client, friendship, receiver.revision, confirming !== null && operation === "set");
  const preview = grants?.preview;
  const [choice, setChoice] = useState<"keep" | "adopt" | null>(null);
  const [scope, setScope] = useState<string | null>(null);
  const scopeConflict = choice === "adopt" && preview?.previous?.machine_access === "full"
    && /read[-_ ]only/i.test(scope ?? preview.previous.scope);
  const label = operation === "reset" ? "Stop receiving this friend" : "Receive this friend here";
  const close = () => { setConfirming(null); onClose?.(); };
  if (confirming === null) {
    return <div className={operation === "reset" ? "mt-3 flex flex-col items-start pl-[calc(2rem+var(--neighbor-space-2,0.75rem))]" : "neighbors-receive"}>{operation === "set" && !receiver.receiving_here && receiver.receiving_enabled && <p className="neighbor-note">{friendName} is received on {receiver.receiver_machine_label ?? "another machine"}. Move here?</p>}<Button size="sm" variant="ghost" className={operation === "reset" ? "!px-0" : undefined} onClick={() => { setError(null); setConfirming(binding); }}>{label}</Button></div>;
  }
  const stale = confirming !== binding;
  return <div role="group" aria-label={`Receive ${friendName} on this machine`} className="ct-card ct-card--static neighbors-receive neighbors-receive--confirm">
    {stale && <p className="neighbor-note">Receiving changed. Confirm again to receive here.</p>}
    <p className="ct-card__meta">{takeoverWarning(receiver, targetMachine, friendName, operation)}</p>
    {operation === "set" && <>
      {!grants && <p role="status">Reading permissions on both machines…</p>}
      {grants?.error && <p role="alert">{grants.error}</p>}
      {preview && <>
        <p>{targetMachine}: {grantSummary(preview.local)}</p>
        <p>Sharing scope: {preview.local.scope || "Nothing allowed"}</p>
        {preview.previous && <><p>{preview.previous_machine ?? "Previous machine"}: {grantSummary(preview.previous)}</p><p>Sharing scope: {preview.previous.scope || "Nothing allowed"}</p></>}
        {preview.unavailable && <p>Previous receiver permissions are unavailable. Only this machine's grant can be kept.</p>}
        {preview.different && <fieldset disabled={busy}><legend>Choose permissions for {targetMachine}</legend>
          <label><input type="radio" name={`grant-${friendship}`} checked={choice === "keep"} onChange={() => setChoice("keep")} />Keep {targetMachine}'s grant</label>
          <label><input type="radio" name={`grant-${friendship}`} checked={choice === "adopt"} onChange={() => setChoice("adopt")} />Adopt {preview.previous_machine ?? "previous receiver"}'s grant</label>
          {choice === "adopt" && <label>Sharing scope for adopted access
            <textarea value={scope ?? preview.previous?.scope ?? ""} onChange={event => setScope(event.currentTarget.value)} />
            <span>Reconcile machine access wording with {preview.previous?.machine_access}; retain sharing restrictions.</span>
            {scopeConflict && <span role="alert">This scope says read-only while machine access is Full. Reconcile that wording before Apply.</span>}
          </label>}
        </fieldset>}
        <p>The concierge restarts with a fresh session. The Domios conversation is retained.</p>
      </>}
    </>}
    {error && <InlineErrorNotice description={error} role="alert" />}
    {keyLimited && <Button size="sm" onClick={() => setManaging(value => !value)}>Manage machines</Button>}
    {managing && <MyMachines account="signed_in"
      onSignIn={() => { window.dispatchEvent(new CustomEvent("codeterm:open-panel", { detail: { panel: "settings:account" } })); }}
      onFailure={() => setError(receiverFailureDescription(null))} />}
    <div className="neighbor-confirm__actions">
      <Button size="sm" variant="primary" loading={busy} disabled={scopeConflict || (operation === "set" && (!preview || preview.revision !== receiver.revision || (preview.different && !choice)))} onClick={() => {
        if (stale) { setConfirming(binding); setChoice(null); setScope(null); return; }
        setBusy(true); setError(null); setKeyLimited(false);
        const decision = preview ? { choice: choice ?? "keep", expected_digest: preview.digest, ...(choice === "adopt" ? { scope: scope ?? preview.previous?.scope ?? "" } : {}) } : undefined;
        void (decision ? onReceive(receiver.revision, decision) : onReceive(receiver.revision)).then(close,
          (failure: unknown) => {
            setKeyLimited(failure instanceof NeighborRuntimeFailure && failure.reason === "key_limit");
            setError(receiverFailureDescription(failure));
          }).finally(() => setBusy(false));
      }}>{preview?.different ? "Apply" : label}</Button>
      <Button size="sm" disabled={busy} onClick={close}>Cancel</Button>
    </div>
  </div>;
}
