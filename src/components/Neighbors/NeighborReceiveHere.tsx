import { useState } from "react";
import { Button } from "../shared/Button";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import type { ReceiverState } from "./neighborRuntimeClient";
import { NeighborRuntimeFailure } from "./neighborRuntimeClient";
import MyMachines from "../MeshPanel/AddPeerPanel";
import { receiverFailureDescription } from "./neighborReceiverError";

function takeoverWarning(receiver: ReceiverState, targetMachine: string, friendName: string, operation: "set" | "reset"): string {
  if (operation === "reset") return `Stop receiving ${friendName}'s messages on this machine. Incoming messages wait until a concierge machine is chosen.`;
  if (receiver.receiving_enabled && !receiver.receiving_here) return `${friendName} is received on ${receiver.receiver_machine_label ?? "another machine"}. Move here? Their concierge starts fresh using this machine's permissions.`;
  return `Receive ${friendName}'s messages on ${targetMachine}. Their concierge uses this machine's permissions.`;
}

export function NeighborReceiveHere({ receiver, onReceive, friendName, targetMachine = "This machine", operation = "set", initiallyOpen = false, onClose }: {
  receiver: ReceiverState;
  onReceive: (revision: number) => Promise<void>;
  friendName: string;
  targetMachine?: string;
  operation?: "set" | "reset";
  initiallyOpen?: boolean;
  onClose?: () => void;
}) {
  const [confirming, setConfirming] = useState<number | null>(initiallyOpen ? receiver.revision : null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<ReturnType<typeof receiverFailureDescription> | null>(null);
  const [keyLimited, setKeyLimited] = useState(false);
  const [managing, setManaging] = useState(false);
  const label = operation === "reset" ? "Stop receiving this friend" : "Receive this friend here";
  const close = () => { setConfirming(null); onClose?.(); };
  if (confirming === null) {
    return <div className="neighbors-receive">{operation === "set" && !receiver.receiving_here && receiver.receiving_enabled && <p className="neighbor-note">{friendName} is received on {receiver.receiver_machine_label ?? "another machine"}. Move here?</p>}<Button size="sm" onClick={() => { setError(null); setConfirming(receiver.revision); }}>{label}</Button></div>;
  }
  const stale = confirming !== receiver.revision;
  return <div role="group" aria-label={`Receive ${friendName} on this machine`} className="ct-card ct-card--static neighbors-receive neighbors-receive--confirm">
    {stale && <p className="neighbor-note">Receiving changed. Confirm again to receive here.</p>}
    <p className="ct-card__meta">{takeoverWarning(receiver, targetMachine, friendName, operation)}</p>
    {error && <InlineErrorNotice description={error} role="alert" />}
    {keyLimited && <Button size="sm" onClick={() => setManaging(value => !value)}>Manage machines</Button>}
    {managing && <MyMachines account="signed_in"
      onSignIn={() => { window.dispatchEvent(new CustomEvent("codeterm:open-panel", { detail: { panel: "settings:account" } })); }}
      onFailure={() => setError(receiverFailureDescription(null))} />}
    <div className="neighbor-confirm__actions">
      <Button size="sm" variant="primary" loading={busy} onClick={() => {
        if (stale) { setConfirming(receiver.revision); return; }
        setBusy(true); setError(null); setKeyLimited(false);
        void onReceive(receiver.revision).then(close,
          (failure: unknown) => {
            setKeyLimited(failure instanceof NeighborRuntimeFailure && failure.reason === "key_limit");
            setError(receiverFailureDescription(failure));
          }).finally(() => setBusy(false));
      }}>{label}</Button>
      <Button size="sm" disabled={busy} onClick={close}>Cancel</Button>
    </div>
  </div>;
}
