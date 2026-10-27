import { InlineErrorNotice } from "../shared/ErrorNotice";
import { useId, useState } from "react";
import { Button } from "../shared/Button";
import { UI_INPUT } from "../shared/design";
import type { NeighborRuntimeClient } from "./neighborRuntimeClient";
import { useNeighborChannel } from "./useNeighborChannel";

export type NeighborChannelState = ReturnType<typeof useNeighborChannel>;

export function NeighborChannel({ owner, friendship, locked, client }: {
  owner: string; friendship: string; locked: boolean; client: NeighborRuntimeClient;
}) {
  return <NeighborChannelView channel={useNeighborChannel(owner, friendship, client)} friendship={friendship} locked={locked} />;
}

const DELIVERY_LABEL = { delivered: "Delivered", pending: "Pending", not_delivered: "Not delivered" } as const;

export function NeighborChannelView({ channel, friendship, locked }: { channel: NeighborChannelState; friendship: string; locked: boolean }) {
  const [confirmTakeover, setConfirmTakeover] = useState<number | null>(null);
  const [confirmRotation, setConfirmRotation] = useState(false);
  const [body, setBody] = useState("");
  const field = useId();
  const tooLarge = new TextEncoder().encode(body).length > 8176;
  return <section aria-label="Encrypted Neighbor channel" className="neighbor-channel">
    {channel.error && <InlineErrorNotice error={channel.error} operation="using your Neighbor channel" role="alert" />}
    {!channel.status && <Button type="button" size="sm" disabled={channel.pending} onClick={() => { void channel.retry(); }}>Refresh receiving status</Button>}
    {channel.status && <p role="status" className="neighbor-note">{channel.status.receiving_here ? "Receiving here" : channel.status.receiving_enabled ? `Receiving on ${channel.status.receiver_machine_label ?? "another machine"}` : channel.status.revision === 0 ? "Receiver not chosen yet" : "Not receiving"}</p>}
    {!locked && channel.status && !channel.status.receiving_here && (confirmTakeover !== null ? <div role="group" aria-label="Receive on this machine" className="neighbor-confirm">
      {confirmTakeover !== channel.status.revision && <p>Receiving changed. Confirm again to receive here.</p>}
      <p>Receive Neighbors on this machine. Other machines will stop receiving.</p>
      <div className="neighbor-confirm__actions">
        <Button type="button" size="sm" variant="primary" disabled={channel.pending} onClick={() => { if (confirmTakeover !== channel.status!.revision) { setConfirmTakeover(channel.status!.revision); return; } setConfirmTakeover(null); void channel.enroll(confirmTakeover); }}>Confirm this machine</Button>
        <Button type="button" size="sm" onClick={() => setConfirmTakeover(null)}>Cancel</Button>
      </div>
    </div> : <Button type="button" size="sm" variant="accent" onClick={() => setConfirmTakeover(channel.status!.revision)}>Receive here</Button>)}
    {channel.status?.receiving_here && <>
      <p role="status" className="neighbor-status"><span className="neighbor-dot" data-tone={channel.reception === "receiving" ? "ok" : channel.reception === "stopped" ? "muted" : "progress"} aria-hidden="true" />Reception: {channel.reception}</p>
      <p className="neighbor-note">{channel.status.bindings[friendship] ? "Incoming messages go to your gateway agent." : "Ask your General Agent to choose a gateway agent for incoming messages."}</p>
      {!locked && <div className="neighbor-confirm__actions">
        <Button type="button" size="sm" disabled={channel.pending} onClick={() => { void channel.retry(); }}>Retry reception</Button>
        {!confirmRotation && <Button type="button" size="sm" disabled={channel.pending} onClick={() => setConfirmRotation(true)}>Rotate keys</Button>}
      </div>}
      {!locked && confirmRotation && <div role="group" aria-label="Rotate Neighbor keys" className="neighbor-confirm">
        <p>Rotate keys? Messages waiting for the previous key will become not delivered.</p>
        <div className="neighbor-confirm__actions">
          <Button type="button" size="sm" variant="primary" disabled={channel.pending} onClick={() => { setConfirmRotation(false); void channel.rotate(); }}>Confirm rotation</Button>
          <Button type="button" size="sm" onClick={() => setConfirmRotation(false)}>Keep keys</Button>
        </div>
      </div>}
    </>}
    {!locked && <>
      {!channel.permission?.allowed && <p className="neighbor-note">Your General Agent must grant a scope before this channel can send.</p>}
      <label htmlFor={field} className="neighbor-label">Message to Neighbor</label>
      <textarea id={field} className={`${UI_INPUT.full} neighbor-textarea`} rows={3} value={body} maxLength={8176} onChange={event => setBody(event.currentTarget.value)} />
      {tooLarge && <p role="alert" className="neighbor-note">The message exceeds the encrypted channel limit.</p>}
      <div className="neighbor-confirm__actions">
        <Button type="button" size="sm" variant="primary" disabled={channel.pending || !channel.status?.enrolled || !channel.permission?.allowed || !body.trim() || tooLarge}
          onClick={() => { const message = body; setBody(""); void channel.send(message); }}>Send encrypted message</Button>
      </div>
    </>}
    {channel.receipts.length > 0 && <ul aria-label="Neighbor deliveries" className="neighbor-receipts">{channel.receipts.map(item => <li key={item.message_id}>
      <span className="neighbor-dot" data-tone={item.status === "delivered" ? "ok" : item.status === "pending" ? "progress" : "danger"} aria-hidden="true" />
      <span>{item.status === "not_delivered" && item.reason === "key_revoked" ? "Not delivered: recipient keys were revoked" : DELIVERY_LABEL[item.status]}</span>
      {item.submission === "unconfirmed" && <span className="neighbor-note">Submission unconfirmed</span>}
      {item.status === "pending" && <Button type="button" size="compact" disabled={channel.pending} onClick={() => { void channel.check(item.message_id); }}>Check delivery</Button>}
    </li>)}</ul>}
  </section>;
}
