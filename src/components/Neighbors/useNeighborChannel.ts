import { useEffect, useRef, useState } from "react";
import type { ChannelStatus, NeighborListenerState, NeighborPermission, NeighborReceipt, NeighborRuntimeClient } from "./neighborRuntimeClient";
import { receipt } from "./neighborRuntimeClient";

export function useNeighborChannel(owner: string, friendship: string, client: NeighborRuntimeClient) {
  const [status, setStatus] = useState<ChannelStatus | null>(null);
  const [reception, setReception] = useState<NeighborListenerState>("stopped");
  const [permission, setPermission] = useState<NeighborPermission | null>(null);
  const [receipts, setReceipts] = useState<NeighborReceipt[]>([]);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const generation = useRef(0);
  const inFlight = useRef<number | null>(null);
  const storageKey = `domios.neighbor-deliveries:${owner}:${friendship}`;

  useEffect(() => {
    const current = ++generation.current;
    inFlight.current = null;
    const controller = new AbortController();
    setStatus(null); setPermission(null); setError(null); setReception("stopped");
    try {
      const stored: unknown = JSON.parse(localStorage.getItem(storageKey) ?? "[]");
      setReceipts(Array.isArray(stored) ? stored.slice(-20).map(receipt) : []);
    } catch { setReceipts([]); }
    let unsubscribe: (() => void) | undefined;
    let observedEvent = false;
    void client.subscribe(event => {
      if (generation.current === current && event.friendship_id === friendship) {
        observedEvent = true; setReception(event.status);
      }
    }).then(stop => {
      if (generation.current !== current) { stop(); return; }
      unsubscribe = stop;
      return Promise.all([client.status(controller.signal), client.permission(friendship, controller.signal)])
        .then(([channel, policy]) => {
          if (generation.current === current) {
            setStatus(channel); setPermission(policy);
            if (!observedEvent) setReception(channel.listeners?.[friendship] ?? "stopped");
          }
        });
    }).catch(() => {
      if (generation.current === current && !controller.signal.aborted) setError("Neighbor channel unavailable. Retry reception to try again.");
    });
    return () => { generation.current++; controller.abort(); unsubscribe?.(); };
  }, [client, friendship, storageKey]);

  function save(values: NeighborReceipt[]) {
    const bounded = values.slice(-20);
    setReceipts(bounded);
    try { localStorage.setItem(storageKey, JSON.stringify(bounded)); } catch { /* Delivery metadata is optional local history. */ }
  }

  async function run<T>(action: () => Promise<T>, apply: (value: T) => void) {
    if (inFlight.current !== null) return;
    const current = generation.current;
    inFlight.current = current;
    setPending(true); setError(null);
    try { const value = await action(); if (generation.current === current) apply(value); }
    catch { if (generation.current === current) setError("The Neighbor operation could not be confirmed. Check delivery before sending again."); }
    finally {
      if (inFlight.current === current) inFlight.current = null;
      if (generation.current === current) setPending(false);
    }
  }

  async function enroll() {
    await run(async () => { await client.enroll(); return client.status(); }, setStatus);
  }
  async function rotate() {
    await run(async () => { await client.rotate(); return client.status(); }, setStatus);
  }
  async function send(body: string) {
    await run(() => client.send(friendship, body), result => save([...receipts, result]));
  }
  async function check(id: string) {
    await run(() => client.deliveryStatus(id), result => save(receipts.map(item => item.message_id === id ? { message_id: id, ...result } : item)));
  }
  async function retry() {
    setReception("connecting");
    await run(async () => { await client.retry(friendship); return client.status(); }, setStatus);
  }
  return { reception, retry, status, permission, receipts, pending, error, enroll, rotate, send, check };
}
