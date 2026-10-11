import type { TransportClient } from "../transportClient";
import { createOrchestrationTransport } from "./orchestration";

let client: TransportClient | undefined;
let transport: ReturnType<typeof createOrchestrationTransport> | undefined;
let generation = new AbortController();
const subscriptions = new Set<() => void>();

export function configureOrchestrationTransport(injected: TransportClient): void {
  generation.abort();
  for (const stop of subscriptions) stop();
  generation = new AbortController();
  const signal = generation.signal;
  client = injected;
  transport = createOrchestrationTransport({
    json: (method, path, body, opts) => injected.json(method, path, body, {
      ...opts, signal: opts?.signal ? AbortSignal.any([signal, opts.signal]) : signal,
    }),
  });
}

export function orchestrationTransport(): ReturnType<typeof createOrchestrationTransport> {
  if (!transport) throw new Error("Orchestration transport has not been configured");
  return transport;
}

export function listenOrchestration<T>(event: string, handler: (event: { payload: T }) => void): Promise<() => void> {
  if (!client) return Promise.reject(new Error("Orchestration transport has not been configured"));
  let active = true;
  const off = client.subscribe("/api/events/stream", (name, payload) => {
    if (active && name === event) handler({ payload: payload as T });
  }, { signal: generation.signal });
  const stop = () => {
    if (!active) return;
    active = false;
    subscriptions.delete(stop);
    off();
  };
  subscriptions.add(stop);
  return Promise.resolve(stop);
}
