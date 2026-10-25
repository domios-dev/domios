import { useState } from "react";
import { Button } from "../shared/Button";
import type { NeighborRuntimeClient } from "./neighborRuntimeClient";
import { useNeighborChannel } from "./useNeighborChannel";

export function NeighborChannel({ owner, friendship, locked, client }: {
  owner: string; friendship: string; locked: boolean; client: NeighborRuntimeClient;
}) {
  const channel = useNeighborChannel(owner, friendship, client);
  const [confirmTakeover, setConfirmTakeover] = useState(false);
  const [confirmRotation, setConfirmRotation] = useState(false);
  const [body, setBody] = useState("");
  const tooLarge = new TextEncoder().encode(body).length > 8176;
  return <section aria-label="Encrypted Neighbor channel">
    {channel.error && <p role="alert">{channel.error}</p>}
    {!locked && channel.status && !channel.status.enrolled && (confirmTakeover ? <div role="group" aria-label="Receive on this machine">
      <p>Receive Neighbors on this machine. Other machines will stop receiving.</p>
      <Button type="button" disabled={channel.pending} onClick={() => { setConfirmTakeover(false); void channel.enroll(); }}>Confirm this machine</Button>
      <Button type="button" onClick={() => setConfirmTakeover(false)}>Cancel</Button>
    </div> : <Button type="button" onClick={() => setConfirmTakeover(true)}>Enable encrypted channel</Button>)}
    {channel.status?.enrolled && <>
      <p>{channel.status.bindings[friendship] ? "Incoming messages go to your gateway agent." : "Ask your General Agent to choose a gateway agent for incoming messages."}</p>
      {!locked && (confirmRotation ? <div role="group" aria-label="Rotate Neighbor keys">
        <p>Rotate keys? Messages waiting for the previous key will become not delivered.</p>
        <Button type="button" disabled={channel.pending} onClick={() => { setConfirmRotation(false); void channel.rotate(); }}>Confirm rotation</Button>
        <Button type="button" onClick={() => setConfirmRotation(false)}>Keep keys</Button>
      </div> : <Button type="button" disabled={channel.pending} onClick={() => setConfirmRotation(true)}>Rotate keys</Button>)}
    </>}
    {!locked && <>
      {!channel.permission?.allowed && <p>Your General Agent must grant a scope before this channel can send.</p>}
      <label>Message to Neighbor<textarea value={body} maxLength={8176} onChange={event => setBody(event.currentTarget.value)} /></label>
      {tooLarge && <p role="alert">The message exceeds the encrypted channel limit.</p>}
      <Button type="button" disabled={channel.pending || !channel.status?.enrolled || !channel.permission?.allowed || !body.trim() || tooLarge}
        onClick={() => { const message = body; setBody(""); void channel.send(message); }}>Send encrypted message</Button>
    </>}
    <ul aria-label="Neighbor deliveries">{channel.receipts.map(item => <li key={item.message_id}>
      <span>{item.status === "not_delivered" ? "Not delivered" : item.status === "delivered" ? "Delivered" : "Pending"}</span>
      {item.submission === "unconfirmed" && <span>Submission unconfirmed</span>}
      {item.status === "pending" && <Button type="button" disabled={channel.pending} onClick={() => { void channel.check(item.message_id); }}>Check delivery</Button>}
    </li>)}</ul>
  </section>;
}
