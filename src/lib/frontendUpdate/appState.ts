import type { ClientCheckpoint } from "./checkpoint";
import { createFrontendSignals } from "./signals";
import type { Safety } from "./model";

const empty: ClientCheckpoint = { activeWorkspaceId: null, activeTabId: null, drafts: {}, scroll: {} };
let restoreCheckpoint: (checkpoint: ClientCheckpoint) => ClientCheckpoint | void = () => {};
export const frontendSignals = createFrontendSignals(empty, {
  focused: false, idle: false, typing: false, composing: false, modalOpen: false, pendingUploads: 0,
}, checkpoint => restoreCheckpoint(checkpoint));

export function setCheckpointRestorer(restore: typeof restoreCheckpoint): void { restoreCheckpoint = restore; }

export function editFrontendDraft(scope: string, text: string): void {
  frontendSignals.editDraft(scope, text);
}

export function editFrontendScroll(key: string, surface: "chat" | "terminal", value: string): void {
  frontendSignals.editScroll(key, surface, value);
}

const uploads = new Map<object, number>();
export function signalFrontendUploads(owner: object, pending: number): void {
  if (pending) uploads.set(owner, pending); else uploads.delete(owner);
  frontendSignals.signal({ pendingUploads: [...uploads.values()].reduce((sum, count) => sum + count, 0) });
}

export function signalFrontendSafety(safety: Partial<Safety>): void { frontendSignals.signal(safety); }
