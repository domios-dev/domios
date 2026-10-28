import { useState } from "react";
import { Button } from "../shared/Button";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import type { ReceiverState } from "./neighborRuntimeClient";
import { NeighborRuntimeFailure } from "./neighborRuntimeClient";
import MyMachines from "../MeshPanel/AddPeerPanel";
import { receiverFailureDescription } from "./neighborReceiverError";

function takeoverWarning(receiver: ReceiverState, targetMachine: string, friendName: string | undefined, operation: "set" | "reset", friendCount: number): string {
  if (friendName !== undefined) {
    if (operation === "reset") return `Use the account receiver for ${friendName} instead of their individual receiver. Only ${friendName}'s messages move.`;
    const source = receiver.receiving_enabled
      ? `${receiver.receiver_machine_label ?? "The other machine"} will stop receiving messages from ${friendName}.`
      : `This machine will start receiving messages from ${friendName}.`;
    return `${source} Receive ${friendName}'s messages on ${targetMachine}. Their concierge starts fresh using this machine's permissions.`;
  }
  const perMachine = "Concierges and permissions are per machine: friends without a concierge on this machine will wait until you set their permissions here.";
  if (!receiver.receiving_enabled) return `Receive all ${friendCount} friends on ${targetMachine}. ${perMachine}`;
  return `${receiver.receiver_machine_label ?? "The other machine"} will stop receiving Neighbor messages. Receive all ${friendCount} friends on ${targetMachine}. ${perMachine}`;
}

export function NeighborReceiveHere({ receiver, onReceive, friendName, targetMachine = "This machine", friendCount = 0, operation = "set", initiallyOpen = false, onClose }: {
  receiver: ReceiverState;
  onReceive: (revision: number) => Promise<void>;
  friendName?: string;
  targetMachine?: string;
  friendCount?: number;
  operation?: "set" | "reset";
  initiallyOpen?: boolean;
  onClose?: () => void;
}) {
  const [confirming, setConfirming] = useState<number | null>(initiallyOpen ? receiver.revision : null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<ReturnType<typeof receiverFailureDescription> | null>(null);
  const [keyLimited, setKeyLimited] = useState(false);
  const [managing, setManaging] = useState(false);
  const label = friendName === undefined ? "Receive all friends here" : operation === "reset" ? "Use account receiver" : "Receive this friend here";
  const close = () => { setConfirming(null); onClose?.(); };
  if (confirming === null) {
    return <div className="neighbors-receive"><Button size="sm" onClick={() => { setError(null); setConfirming(receiver.revision); }}>{label}</Button></div>;
  }
  const stale = confirming !== receiver.revision;
  return <div role="group" aria-label={friendName === undefined ? "Receive on this machine" : `Receive ${friendName} on this machine`} className="ct-card ct-card--static neighbors-receive neighbors-receive--confirm">
    {stale && <p className="neighbor-note">Receiving changed. Confirm again to receive here.</p>}
    <p className="ct-card__meta">{takeoverWarning(receiver, targetMachine, friendName, operation, friendCount)}</p>
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
