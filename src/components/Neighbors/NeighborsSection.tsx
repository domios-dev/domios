import { useId, useState } from "react";
import { useAccount } from "../Settings/useAccount";
import { Button } from "../shared/Button";
import { FormDialog } from "../shared/FormDialog";
import { SidebarDisclosure } from "../Sidebar/SidebarDisclosure";
import { neighborName, validNeighborLimit, type Neighbor } from "./neighborModel";
import { useNeighbors } from "./useNeighbors";
import type { NeighborClient } from "./neighborClient";
import { NeighborChannel } from "./NeighborChannel";
import { neighborRuntimeClient, type NeighborRuntimeClient } from "./neighborRuntimeClient";

const openPlans = () => window.dispatchEvent(new CustomEvent("codeterm:open-panel", { detail: { panel: "settings:plans" } }));

export function NeighborDetails({ neighbor, locked, pending, onClose, onLimit, onRemove, onOpenGeneral, owner, runtimeClient }: {
  neighbor: Neighbor; locked: boolean; pending: boolean; onClose: () => void;
  onLimit: (limit: number) => Promise<boolean>; onRemove: () => Promise<boolean>; onOpenGeneral: () => void;
  owner?: string; runtimeClient?: NeighborRuntimeClient;
}) {
  const title = useId();
  const [limit, setLimit] = useState(neighbor.inbound_per_hour);
  const [confirmRemove, setConfirmRemove] = useState(false);
  return <FormDialog titleId={title} onCancel={onClose} onSubmit={event => {
    event.preventDefault();
    if (validNeighborLimit(limit)) void onLimit(limit);
  }}>
    <h2 id={title}>{neighborName(neighbor)}</h2>
    {locked && <div role="status">Your Neighbors channel is locked by your plan. You can keep and manage this connection.
      <Button type="button" onClick={openPlans}>View plans</Button>
    </div>}
    <p>Connected since {new Date(neighbor.since * 1000).toLocaleDateString()}.</p>
    <p>Ask your General Agent to review permissions before sharing any information with this Neighbor.</p>
    <Button type="button" onClick={() => { onClose(); onOpenGeneral(); }}>Open General Agent</Button>
    {owner && runtimeClient && <NeighborChannel owner={owner} friendship={neighbor.friendship_id} locked={locked} client={runtimeClient} />}
    <label>Incoming requests per hour
      <input type="number" min={1} max={200} step={1} value={limit} onChange={event => setLimit(event.currentTarget.valueAsNumber)} />
    </label>
    <p>This Neighbor accepts up to {neighbor.outbound_per_hour} requests per hour from you.</p>
    <Button type="submit" disabled={pending || !validNeighborLimit(limit)}>Save limit</Button>
    {confirmRemove ? <div role="group" aria-label="Confirm removal">
      <p>Remove this Neighbor? Their channel access will end.</p>
      <Button type="button" disabled={pending} onClick={() => { void onRemove().then(removed => { if (removed) onClose(); }); }}>Confirm remove</Button>
      <Button type="button" disabled={pending} onClick={() => setConfirmRemove(false)}>Keep Neighbor</Button>
    </div> : <Button type="button" disabled={pending} onClick={() => setConfirmRemove(true)}>Remove Neighbor</Button>}
    <Button type="button" onClick={onClose}>Close</Button>
  </FormDialog>;
}

export function NeighborsInventory({ identity, onOpenGeneral, client, runtimeClient }: {
  identity: string | null; onOpenGeneral: () => void; client?: NeighborClient;
  runtimeClient?: NeighborRuntimeClient;
}) {
  const neighbors = useNeighbors(identity, client);
  const [expanded, setExpanded] = useState(false);
  const [adding, setAdding] = useState(false);
  const [email, setEmail] = useState("");
  const [sent, setSent] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const contentId = useId();
  const title = useId();
  const selected = neighbors.list?.friends.find(friend => friend.friendship_id === selectedId);
  const paid = neighbors.list?.access === "paid";
  return <section aria-label="Neighbors" className="sidebar-team-section min-w-0">
    <SidebarDisclosure expanded={expanded} aria-controls={contentId} onClick={() => { setExpanded(value => !value); setAdding(false); setSelectedId(null); }}>Neighbors</SidebarDisclosure>
    <div id={contentId} hidden={!expanded}>
      {!identity ? <p>Sign in to manage your Neighbors.</p> : <>
        {neighbors.error && <p role="alert">{neighbors.error}</p>}
        {neighbors.list === null && !neighbors.error && <p role="status">Loading Neighbors…</p>}
        <Button type="button" disabled={neighbors.pending} onClick={neighbors.refresh}>Refresh Neighbors</Button>
        {paid ? <Button type="button" onClick={() => { setAdding(true); setSent(false); }}>Add Neighbor</Button>
          : neighbors.list && <Button type="button" onClick={openPlans}>Upgrade to add Neighbors</Button>}
        <ul aria-label="Neighbor requests">{neighbors.list?.incoming.map(request => <li key={request.request_id}>
          <span>{neighborName(request)}</span>
          <Button type="button" disabled={neighbors.pending} onClick={() => { void neighbors.apply({ operation: "respond", request_id: request.request_id, accept: true }); }}>Accept {neighborName(request)}</Button>
          <Button type="button" disabled={neighbors.pending} onClick={() => { void neighbors.apply({ operation: "respond", request_id: request.request_id, accept: false }); }}>Decline {neighborName(request)}</Button>
        </li>)}</ul>
        <ul aria-label="Your Neighbors">{neighbors.list?.friends.map(friend => <li key={friend.friendship_id}>
          <Button type="button" onClick={() => setSelectedId(friend.friendship_id)}>{neighborName(friend)}</Button>
        </li>)}</ul>
        {selected && <NeighborDetails key={selected.friendship_id} neighbor={selected} locked={!paid} pending={neighbors.pending}
          owner={identity ?? undefined} runtimeClient={runtimeClient}
          onClose={() => setSelectedId(null)} onOpenGeneral={onOpenGeneral}
          onLimit={per_hour => neighbors.apply({ operation: "set_limit", friendship_id: selected.friendship_id, per_hour })}
          onRemove={() => neighbors.apply({ operation: "remove", friendship_id: selected.friendship_id })} />}
        {adding && <FormDialog titleId={title} onCancel={() => setAdding(false)} onSubmit={event => {
          event.preventDefault();
          void neighbors.apply({ operation: "request", email: email.trim() }).then(ok => { if (ok) setSent(true); });
        }}>
          <h2 id={title}>Add Neighbor</h2>
          <label>Email address<input type="email" required maxLength={254} value={email} onChange={event => { setEmail(event.currentTarget.value); setSent(false); }} /></label>
          <p>Enter their Domios account email. Requests and invitations receive the same confirmation to protect account privacy.</p>
          {neighbors.error && <p role="alert">{neighbors.error}</p>}
          {sent && <p role="status">Request or invitation saved.</p>}
          <Button type="submit" disabled={neighbors.pending || sent || email.trim().length === 0}>Send request or invitation</Button>
          <Button type="button" onClick={() => setAdding(false)}>Close</Button>
        </FormDialog>}
      </>}
    </div>
  </section>;
}

export function NeighborsSection({ onOpenGeneral }: { onOpenGeneral: () => void }) {
  const { model } = useAccount();
  const identity = model.kind === "signed_in" ? model.identity.uid : null;
  return <NeighborsInventory key={identity} identity={identity} onOpenGeneral={onOpenGeneral} runtimeClient={neighborRuntimeClient} />;
}
