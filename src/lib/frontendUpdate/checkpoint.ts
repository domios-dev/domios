export interface Draft {
  text: string;
  attachmentIds: string[];
  replyTo: string | null;
}

export interface ClientCheckpoint {
  activeWorkspaceId: string | null;
  activeTabId: string | null;
  drafts: Record<string, Draft>;
  scroll: Record<string, { terminal: string | null; chat: string | null }>;
}

export interface CheckpointWrite {
  revision: number;
  payload: ClientCheckpoint;
}

export function checkpointWrite(revision: number, payload: ClientCheckpoint): CheckpointWrite {
  if (!Number.isSafeInteger(revision) || revision < 0) throw new Error("Invalid checkpoint revision");
  return { revision, payload: structuredClone(payload) };
}

export function checkpointAcknowledged(expected: CheckpointWrite, received: CheckpointWrite): boolean {
  return expected.revision === received.revision && equalValue(expected.payload, received.payload);
}

function equalValue(left: unknown, right: unknown): boolean {
  if (Object.is(left, right)) return true;
  if (!left || !right || typeof left !== "object" || typeof right !== "object") return false;
  if (Array.isArray(left) || Array.isArray(right)) {
    return Array.isArray(left) && Array.isArray(right) && left.length === right.length
      && left.every((value, index) => equalValue(value, right[index]));
  }
  const a = left as Record<string, unknown>;
  const b = right as Record<string, unknown>;
  const keys = Object.keys(a);
  return keys.length === Object.keys(b).length
    && keys.every(key => Object.prototype.hasOwnProperty.call(b, key) && equalValue(a[key], b[key]));
}
