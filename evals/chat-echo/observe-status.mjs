export function claimedRowsComplete(statuses, sendIds) {
  const selected = sendIds.map(id => statuses.find(status => status.send_id === id));
  return new Set(sendIds).size === sendIds.length && selected.length > 0
    && selected.every(status => status?.state === 'delivered' && status.matched_row)
    && new Set(selected.map(status => JSON.stringify(status.matched_row))).size === sendIds.length;
}

export async function observeSendClaims({ tabId, sendIds, events, readLedger, signal, persist }) {
  const statuses = new Map();
  const receipt = { status: 'incomplete', initialLedger: null, finalLedger: null, events: [] };
  const owned = status => status?.tab_id === tabId && sendIds.includes(status.send_id);
  const update = rows => rows.filter(owned).forEach(status => statuses.set(status.send_id, status));
  const complete = () => claimedRowsComplete([...statuses.values()], sendIds);
  try {
    receipt.initialLedger = await readLedger(signal);
    update(receipt.initialLedger);
    persist(receipt);
    while (!complete()) {
      signal.throwIfAborted();
      const event = await events.next();
      if (event.event_type !== 'chat-send-status' || !owned(event.data)) continue;
      update([event.data]);
      receipt.events.push(event.data);
      if (receipt.events.length > 256) throw Error('Owned status observation exceeded 256 events');
      persist(receipt);
    }
  } catch (error) {
    receipt.error = String(error);
  } finally {
    try {
      receipt.finalLedger = await readLedger(AbortSignal.timeout(10_000));
      receipt.status = claimedRowsComplete(receipt.finalLedger.filter(owned), sendIds) ? 'completed' : 'incomplete';
    } catch (error) {
      receipt.finalLedgerError = String(error);
    }
    receipt.observedStatuses = [...statuses.values()];
    persist(receipt);
  }
  return receipt;
}
