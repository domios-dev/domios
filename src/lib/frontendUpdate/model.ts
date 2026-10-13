import { checkpointAcknowledged, checkpointWrite, type CheckpointWrite, type ClientCheckpoint } from "./checkpoint";

export interface Release {
  releaseId: string;
  version: string;
  minApiVersion: number;
  maxApiVersion: number;
}

export interface Safety {
  focused: boolean;
  idle: boolean;
  typing: boolean;
  composing: boolean;
  modalOpen: boolean;
  pendingUploads: number;
}

export interface Ticket {
  releaseId: string;
  generation: number;
  token: string;
}

export interface SwapState {
  apiVersion: number;
  safety: Safety;
  pending: Release | null;
  revision: number;
  checkpoint: ClientCheckpoint;
  acknowledgedRevision: number | null;
  writing: CheckpointWrite | null;
  activating: string | null;
  ticket: Ticket | null;
  reloading: boolean;
  failed: boolean;
  consented: boolean;
  held: boolean;
}

export type SwapEvent =
  | { type: "ready"; release: Release }
  | { type: "safety"; safety: Safety }
  | { type: "edited"; checkpoint: ClientCheckpoint }
  | { type: "persisted"; write: CheckpointWrite }
  | { type: "activated"; ticket: Ticket }
  | { type: "failed" }
  | { type: "consent" }
  | { type: "hold"; held: boolean }
  | { type: "rolledBack"; releaseId: string };

export type SwapEffect =
  | { type: "persist"; write: CheckpointWrite }
  | { type: "activate"; releaseId: string }
  | { type: "reload"; ticket: Ticket };

export function compatible(release: Release, api: number): boolean {
  return Number.isSafeInteger(api) && api > 0
    && Number.isSafeInteger(release.minApiVersion) && release.minApiVersion > 0
    && Number.isSafeInteger(release.maxApiVersion)
    && release.minApiVersion <= api && api <= release.maxApiVersion;
}

// Nobody looking: swap at once. Someone looking: swap only after they choose to refresh.
export function safeToSwap(safety: Safety, consented = false): boolean {
  return safety.pendingUploads === 0 && (!safety.focused || consented);
}

export function initialSwapState(apiVersion: number, checkpoint: ClientCheckpoint, safety: Safety, restoredRevision = 0): SwapState {
  checkpointWrite(restoredRevision, checkpoint);
  return { apiVersion, checkpoint: structuredClone(checkpoint), safety, pending: null,
    revision: restoredRevision, acknowledgedRevision: null, writing: null, activating: null,
    ticket: null, reloading: false, failed: false, consented: false, held: false };
}

export function swapEvent(current: SwapState, event: SwapEvent): { state: SwapState; effects: SwapEffect[] } {
  if (current.reloading) return { state: current, effects: [] };
  const state = { ...current };
  switch (event.type) {
    case "ready":
      if (compatible(event.release, state.apiVersion) && !state.activating && !state.ticket) {
        if (state.pending?.releaseId !== event.release.releaseId) state.consented = false;
        state.pending = event.release;
        state.failed = false;
      }
      break;
    case "hold": state.held = event.held; break;
    case "consent":
      if (state.pending) {
        state.consented = true;
        state.failed = false;
      }
      break;
    case "safety": state.safety = event.safety; break;
    case "edited":
      state.checkpoint = structuredClone(event.checkpoint);
      state.revision += 1;
      break;
    case "persisted":
      if (!state.writing || !checkpointAcknowledged(state.writing, event.write)) break;
      state.acknowledgedRevision = event.write.revision;
      state.writing = null;
      break;
    case "activated":
      if (state.activating !== event.ticket.releaseId || state.pending?.releaseId !== event.ticket.releaseId) break;
      state.ticket = event.ticket;
      state.activating = null;
      break;
    case "failed":
      state.failed = true;
      state.writing = null;
      state.activating = null;
      break;
    case "rolledBack":
      if (state.pending?.releaseId === event.releaseId) {
        state.pending = null;
        state.ticket = null;
        state.activating = null;
      }
      break;
  }
  const effects: SwapEffect[] = [];
  if (!state.held && !state.failed && state.pending && safeToSwap(state.safety, state.consented)) {
    if (state.acknowledgedRevision !== state.revision) {
      if (!state.writing) {
        state.writing = checkpointWrite(state.revision, state.checkpoint);
        effects.push({ type: "persist", write: state.writing });
      }
    } else if (state.ticket) {
      state.reloading = true;
      effects.push({ type: "reload", ticket: state.ticket });
    } else if (!state.activating) {
      state.activating = state.pending.releaseId;
      effects.push({ type: "activate", releaseId: state.pending.releaseId });
    }
  }
  return { state, effects };
}

export interface BootObservation {
  ticket: Ticket;
  deadlineMs: number;
  rendered: boolean;
  failed: boolean;
}

export function observeBoot(state: BootObservation, event: { type: "rootRendered" | "exception" | "deadline"; nowMs: number }): { state: BootObservation; report: "healthy" | "rollback" | null } {
  if (state.failed) return { state, report: null };
  if (event.type === "exception" || (event.type === "deadline" && !state.rendered && event.nowMs >= state.deadlineMs)) {
    return { state: { ...state, failed: true }, report: "rollback" };
  }
  if (event.type === "rootRendered" && !state.rendered && event.nowMs < state.deadlineMs) {
    return { state: { ...state, rendered: true }, report: "healthy" };
  }
  return { state, report: null };
}

export function recoveryTarget(previous: Release | null, bundled: Release, api: number, attempted: readonly string[], maxAttempts = 3): Release | null {
  if (attempted.length >= maxAttempts) return null;
  return [previous, bundled].find((release): release is Release =>
    release !== null && compatible(release, api) && !attempted.includes(release.releaseId)) ?? null;
}
