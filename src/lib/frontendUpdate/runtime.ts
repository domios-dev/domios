import type { TransportClient } from "../transportClient";
import { createFrontendUpdateClient, createSwapController, preparePageReload, type BootContext } from "./client";
import type { ClientCheckpoint } from "./checkpoint";
import { initialSwapState, type Release, type Safety } from "./model";

export const ROOT_RENDERED_EVENT = "codeterm:frontend-root-rendered";
export const BOOT_CONTEXT_KEY = "codeterm.frontend.boot";

export interface FrontendSignals {
  checkpoint(): ClientCheckpoint;
  hydrate(checkpoint: ClientCheckpoint): void;
  safety(): Safety;
  subscribe(listener: (change: "checkpoint" | "safety") => void): () => void;
}

interface FrontendStatus {
  apiVersion: number;
  pending: Release | null;
  quarantined: string[];
}

export interface FrontendRuntimeOptions {
  clientId: string;
  signals: FrontendSignals;
  storage: Pick<Storage, "getItem" | "setItem">;
  navigate(path: string): void;
  events: Pick<EventTarget, "dispatchEvent">;
  event(detail: { releaseId: string; generation: number }): Event;
  boot: BootContext | null;
  bundled: BootContext["bundled"];
  onError(error: unknown): void;
}

export async function initializeFrontendUpdateRuntime(client: TransportClient, options: FrontendRuntimeOptions) {
  const api = createFrontendUpdateClient({ json: (method, path, body) => client.localJson(method, path, body) }, options.clientId);
  const restored = await api.checkpoint();
  if (restored) options.signals.hydrate(restored.payload);
  const status = await client.localJson<FrontendStatus>("GET", "/api/frontend/status");
  const controller = createSwapController(initialSwapState(status.apiVersion, options.signals.checkpoint(), options.signals.safety(), restored?.revision ?? 0), api, ticket => {
    const token = client.currentToken();
    if (!token) { options.onError(new Error("Frontend reload requires daemon credentials")); return; }
    preparePageReload({ apiOrigin: client.origin(), token, clientId: options.clientId, ticket,
      attempted: options.boot?.attempted ?? [], bundled: options.bundled }, options.storage, options.navigate);
  });
  const stopSignals = options.signals.subscribe(change => {
    if (change === "checkpoint") controller.dispatch({ type: "edited", checkpoint: options.signals.checkpoint() });
    else controller.dispatch({ type: "safety", safety: options.signals.safety() });
  });
  const updateStatus = (next: FrontendStatus) => {
    for (const releaseId of next.quarantined) controller.dispatch({ type: "rolledBack", releaseId });
    if (next.pending) controller.dispatch({ type: "ready", release: next.pending });
  };
  let disposed = false;
  let streamObserved = false;
  const reconcile = () => {
    void client.localJson<FrontendStatus>("GET", "/api/frontend/status").then(next => { if (!disposed) updateStatus(next); }, options.onError);
  };
  const stopStream = client.subscribe("/api/events/stream", (name, data) => {
    if (name === "frontend-update-ready") controller.dispatch({ type: "ready", release: data as Release });
    else if (name === "frontend-status") updateStatus(data as FrontendStatus);
    else if (name === "overflow") reconcile();
  }, {
    onActivity: () => { if (!streamObserved) { streamObserved = true; reconcile(); } },
    onEnd: end => { if (end.reason !== "abort" && !disposed) { controller.dispatch({ type: "failed" }); options.onError(new Error(`Frontend event stream ended: ${end.reason}`)); } },
  });
  updateStatus(status);
  let rendered = false;
  return {
    snapshot: controller.snapshot,
    rootRendered() {
      if (rendered) return;
      rendered = true;
      const ticket = options.boot?.ticket;
      if (ticket) options.events.dispatchEvent(options.event({ releaseId: ticket.releaseId, generation: ticket.generation }));
    },
    dispose() { disposed = true; stopStream(); stopSignals(); controller.dispose(); },
  };
}
