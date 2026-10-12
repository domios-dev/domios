import { FrontendUpdateError } from "./bootstrapModel";
import { frontendApiCompatible } from "../frontendApiContract";
import { streamRecoveryAction } from "../streamRecoveryModel";
import { TERMINAL_VIEWER_RECONNECTED_EVENT } from "../terminalViewerEvents";
import type { TransportClient } from "../transportClient";
import { createFrontendUpdateClient, createSwapController, preparePageReload, type BootContext } from "./client";
import type { ClientCheckpoint } from "./checkpoint";
import { initialSwapState, type Release, type Safety, type SwapState } from "./model";

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
  events: Pick<EventTarget, "dispatchEvent" | "addEventListener" | "removeEventListener">;
  event(detail: { releaseId: string; generation: number }): Event;
  boot: BootContext | null;
  bundled: BootContext["bundled"];
  onError(error: unknown): void;
  onOffer?(offer: FrontendUpdateOffer | null): void;
}

export interface FrontendUpdateOffer {
  releaseId: string;
  version: string;
}

export function updateOffer(state: SwapState): FrontendUpdateOffer | null {
  if (!state.pending || state.consented || state.reloading) return null;
  return { releaseId: state.pending.releaseId, version: state.pending.version };
}

export async function initializeFrontendUpdateRuntime(client: TransportClient, options: FrontendRuntimeOptions) {
  const api = createFrontendUpdateClient({ json: (method, path, body) => client.localJson(method, path, body, { signal: AbortSignal.timeout(5000) }) }, options.clientId);
  const restored = await api.checkpoint().catch(error => { throw new FrontendUpdateError("checkpoint-unavailable", error); });
  if (restored) options.signals.hydrate(restored.payload);
  const status = await client.localJson<FrontendStatus>("GET", "/api/frontend/status", undefined, { signal: AbortSignal.timeout(5000) })
    .catch(error => { throw new FrontendUpdateError("status-unavailable", error); });
  if (!frontendApiCompatible(status.apiVersion)) throw new FrontendUpdateError("api-incompatible");
  const controller = createSwapController(initialSwapState(status.apiVersion, options.signals.checkpoint(), options.signals.safety(), restored?.revision ?? 0), api, ticket => {
    const token = client.currentToken();
    if (!token) { options.onError(new Error("Frontend reload requires daemon credentials")); return; }
    preparePageReload({ apiOrigin: client.origin(), token, clientId: options.clientId, ticket,
      attempted: options.boot?.attempted ?? [], bundled: options.bundled }, options.storage, options.navigate);
  }, state => options.onOffer?.(updateOffer(state)));
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
  let stopStream = () => {};
  let waitingForRecovery = false;
  const connect = () => {
    if (disposed) return;
    waitingForRecovery = false;
    streamObserved = false;
    stopStream = client.subscribe("/api/events/stream", (name, data) => {
    if (name === "frontend-update-ready") controller.dispatch({ type: "ready", release: data as Release });
    else if (name === "frontend-status") updateStatus(data as FrontendStatus);
    else if (name === "overflow") reconcile();
  }, {
    onActivity: () => { if (!streamObserved) { streamObserved = true; reconcile(); } },
    onEnd: end => {
      const action = streamRecoveryAction(end.reason, disposed);
      if (action === "reconnect") queueMicrotask(connect);
      else if (action === "wait") waitingForRecovery = true;
    },
    });
  };
  const recovered = () => { if (waitingForRecovery) connect(); };
  options.events.addEventListener(TERMINAL_VIEWER_RECONNECTED_EVENT, recovered);
  connect();
  updateStatus(status);
  let rendered = false;
  return {
    snapshot: controller.snapshot,
    consent() { controller.dispatch({ type: "consent" }); },
    rootRendered() {
      if (rendered) return;
      rendered = true;
      const ticket = options.boot?.ticket;
      if (ticket) options.events.dispatchEvent(options.event({ releaseId: ticket.releaseId, generation: ticket.generation }));
    },
    dispose() { disposed = true; options.events.removeEventListener(TERMINAL_VIEWER_RECONNECTED_EVENT, recovered); stopStream(); stopSignals(); controller.dispose(); },
  };
}
