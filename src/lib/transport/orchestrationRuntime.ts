import type { TransportClient } from "../transportClient";
import { createOrchestrationTransport } from "./orchestration";

let client: TransportClient | undefined;
let transport: ReturnType<typeof createOrchestrationTransport> | undefined;
let generation = new AbortController();
const subscriptions = new Set<() => void>();
type EventListener = (name: string, payload: unknown) => void;
const listeners = new Set<EventListener>();
let closeSharedStream: (() => void) | null = null;

// Browsers allow ~6 HTTP/1.1 connections per daemon origin; one stream per listener exhausts them and stalls every request.
function openSharedStream(): void {
  if (closeSharedStream || !client) return;
  closeSharedStream = client.subscribe("/api/events/stream", (name, payload) => {
    for (const listener of [...listeners]) listener(name, payload);
  }, { signal: generation.signal, onEnd: () => { closeSharedStream = null; } });
}

export function configureOrchestrationTransport(injected: TransportClient): void {
  generation.abort();
  for (const stop of subscriptions) stop();
  closeSharedStream = null;
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
  const listener: EventListener = (name, payload) => {
    if (name === event) handler({ payload: payload as T });
  };
  listeners.add(listener);
  openSharedStream();
  const stop = () => {
    if (!listeners.delete(listener)) return;
    subscriptions.delete(stop);
    if (listeners.size > 0) return;
    closeSharedStream?.();
    closeSharedStream = null;
  };
  subscriptions.add(stop);
  return Promise.resolve(stop);
}

export function bindOrchestrationTransport(client: TransportClient): void {
  configureOrchestrationTransport(client);
}
