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

export type FrontendUpdateConnection = "connected" | "reconnecting" | "unavailable";

export interface SwapState {
  apiVersion: number;
  connection: FrontendUpdateConnection;
  safety: Safety;
  pending: Release | null;
  revision: number;
  checkpoint: CheckpointSnapshot;
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
  | { type: "connection"; status: FrontendUpdateConnection }
  | { type: "safety"; safety: Safety }
  | { type: "edited"; checkpoint: ClientCheckpoint }
  | { type: "editedSnapshot"; checkpoint: CheckpointSnapshot }
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

type DeepReadonly<T> = T extends readonly (infer Value)[]
  ? readonly DeepReadonly<Value>[]
  : T extends object
    ? { readonly [Key in keyof T]: DeepReadonly<T[Key]> }
    : T;

export type CheckpointSnapshot = DeepReadonly<ClientCheckpoint>;

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

export function initialSwapState(apiVersion: number, checkpoint: ClientCheckpoint | CheckpointSnapshot, safety: Safety, restoredRevision = 0): SwapState {
  const initial = checkpointWrite(restoredRevision, checkpoint as unknown as ClientCheckpoint);
  return { apiVersion, checkpoint: initial.payload, safety, pending: null,
    connection: "connected",
    revision: restoredRevision, acknowledgedRevision: null, writing: null, activating: null,
    ticket: null, reloading: false, failed: false, consented: false, held: false };
}

export function swapEvent(current: SwapState, event: SwapEvent): { state: SwapState; effects: SwapEffect[] } {
  if (current.reloading) return { state: current, effects: [] };
  if (event.type === "connection") return { state: { ...current, connection: event.status }, effects: [] };
  const state = { ...current };
  switch (event.type) {
    case "ready":
      if (state.connection === "connected" && compatible(event.release, state.apiVersion) && !state.activating && !state.ticket) {
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
    case "editedSnapshot":
      if (state.checkpoint === event.checkpoint) break;
      state.checkpoint = event.checkpoint;
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
  if (state.connection === "connected" && !state.held && !state.failed && state.pending && safeToSwap(state.safety, state.consented)) {
    if (state.acknowledgedRevision !== state.revision) {
      if (!state.writing) {
        state.writing = checkpointWrite(state.revision, state.checkpoint as ClientCheckpoint);
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

export interface ServerHandoffEvidence {
  operationId: string;
  oldEpoch: string;
  expectedPredecessor: string;
  explicitDecision: boolean;
  stagedPair: "pending" | "compatible" | "incompatible";
  snapshot: "pending" | "persisted" | "failed";
  requestedExit: "pending" | "acknowledged" | "failed";
  exitedPredecessor: string | null;
  replacement: {
    operationId: string;
    predecessorEpoch: string;
    epoch: string;
    provenanceMatches: boolean;
    health: "pending" | "healthy" | "unhealthy";
    sessions: "pending" | "exact" | "failed";
  } | null;
}

export type ServerHandoffStatus =
  | "decision-required"
  | "stage-required"
  | "snapshot-required"
  | "exit-ack-required"
  | "predecessor-exit-required"
  | "replacement-required"
  | "health-required"
  | "session-restore-required"
  | "ready"
  | "rollback"
  | "refused";

export function serverHandoffStatus(evidence: ServerHandoffEvidence): ServerHandoffStatus {
  if (!evidence.operationId || !evidence.oldEpoch || !evidence.expectedPredecessor) return "refused";
  if (!evidence.explicitDecision) return "decision-required";
  if (evidence.stagedPair === "pending") return "stage-required";
  if (evidence.stagedPair === "incompatible") return "refused";
  if (evidence.snapshot === "failed" || evidence.requestedExit === "failed") return "refused";
  if (evidence.snapshot === "pending") return "snapshot-required";
  if (evidence.requestedExit === "pending") return "exit-ack-required";
  if (evidence.exitedPredecessor === null) return "predecessor-exit-required";
  if (evidence.exitedPredecessor !== evidence.expectedPredecessor) return "refused";
  if (!evidence.replacement) return "replacement-required";
  const replacement = evidence.replacement;
  if (replacement.operationId !== evidence.operationId
    || replacement.predecessorEpoch !== evidence.oldEpoch
    || !replacement.epoch
    || replacement.epoch === evidence.oldEpoch
    || !replacement.provenanceMatches) return "refused";
  if (replacement.health === "pending") return "health-required";
  if (replacement.health === "unhealthy" || replacement.sessions === "failed") return "rollback";
  if (replacement.sessions === "pending") return "session-restore-required";
  return "ready";
}

export type ClientReconnectStatus = "reconnecting" | "connected" | "unavailable";

export function clientReconnectStatus(
  previousServerOwner: string,
  attachedServerOwner: string | null,
  compatibleServer: boolean,
  requestedServerShutdown: boolean,
): ClientReconnectStatus {
  if (requestedServerShutdown || !previousServerOwner || !compatibleServer) return "unavailable";
  if (attachedServerOwner === null) return "reconnecting";
  return attachedServerOwner === previousServerOwner ? "connected" : "unavailable";
}

export interface ServerQuitEvidence {
  serverIdentityMatches: boolean;
  challengeMatches: boolean;
  operationMatches: boolean;
  snapshot: "pending" | "persisted" | "failed";
  requestedExit: "pending" | "settled" | "failed";
  processExited: boolean;
}

export type ServerQuitStatus =
  | "refused"
  | "snapshot-required"
  | "exit-settlement-required"
  | "server-exit-required"
  | "client-exit-permitted";

export function serverQuitStatus(evidence: ServerQuitEvidence): ServerQuitStatus {
  if (!evidence.serverIdentityMatches || !evidence.challengeMatches || !evidence.operationMatches
    || evidence.snapshot === "failed" || evidence.requestedExit === "failed") return "refused";
  if (evidence.snapshot === "pending") return "snapshot-required";
  if (evidence.requestedExit === "pending") return "exit-settlement-required";
  return evidence.processExited ? "client-exit-permitted" : "server-exit-required";
}

export type NativeExecutionState = "not-started" | "in-flight" | "uncertain" | "completed";

export function admitUpdateTransaction(activeOperationId: string | null, requestedOperationId: string): {
  operationId: string | null;
  result: "accepted" | "joined" | "busy" | "refused";
} {
  if (!requestedOperationId) return { operationId: activeOperationId, result: "refused" };
  if (activeOperationId === null) return { operationId: requestedOperationId, result: "accepted" };
  return activeOperationId === requestedOperationId
    ? { operationId: activeOperationId, result: "joined" }
    : { operationId: activeOperationId, result: "busy" };
}

export function claimNativeExecution(state: NativeExecutionState): { state: NativeExecutionState; execute: boolean } {
  return state === "not-started" ? { state: "in-flight", execute: true } : { state, execute: false };
}

export function nativeExecutionAfterDisconnect(state: NativeExecutionState): NativeExecutionState {
  return state === "in-flight" ? "uncertain" : state;
}
