import type { ChatSend } from "@codeterm/chat-core";
import { applyDeliveryStatus, deliveryState, type DeliveryDraft, type DeliverySend } from "./chatDeliveryModel";
import type { ChatSendStatus } from "./chatSendStatus";

type BodyRef = number;

interface DurableEcho {
  body: BodyRef;
  timestampMs: number | null;
}

interface DurableSystemSend {
  body: BodyRef;
  sent_at_ms: number;
}

type DurableStatus = Omit<ChatSendStatus, "tab_id" | "system_send"> & {
  system_send?: DurableSystemSend;
};

interface DurableDraft {
  text: BodyRef;
  attachments: string[];
  recordedVoiceAttachments: string[];
  capsules: DeliveryDraft["capsules"];
  reply: DeliveryDraft["reply"];
}

interface DurableSend {
  id: string;
  content: BodyRef;
  sentAtMs: number;
  correlationId?: string;
  echo?: DurableEcho;
  matchedRow?: ChatSend["matchedRow"];
  matchedRowInStatus?: boolean;
  echoBoundary?: ChatSend["echoBoundary"];
  droppedUploads?: string[];
  dismissed?: boolean;
  status?: DurableStatus;
  requestFailed?: boolean;
  requestUncertain?: boolean;
  requestHadResponse?: boolean;
  draft?: DurableDraft;
}

export interface ChatSendDurableRecord {
  format: "codeterm.chat-send";
  version: 1;
  tabId: string;
  sendId: string;
  revision: number;
  order: number;
  deleted?: true;
  bodies: string[];
  send?: DurableSend;
}

export interface ChatSendDurableTombstone extends Omit<ChatSendDurableRecord, "send" | "bodies" | "deleted"> {
  deleted: true;
  bodies: [];
}

export function encodeChatSend(tabId: string, send: DeliverySend, revision = 0, order = 0): ChatSendDurableRecord {
  const bodies: string[] = [];
  const body = (value: string) => {
    const found = bodies.indexOf(value);
    if (found >= 0) return found;
    bodies.push(value);
    return bodies.length - 1;
  };
  const status = send.status ? encodeStatus(send.status, body) : undefined;
  const draft = send.draft;
  const matchedRowInStatus = !!send.matchedRow && !!send.status?.matched_row
    && stableJson(send.matchedRow) === stableJson(send.status.matched_row);
  const encoded: DurableSend = {
    id: send.id,
    content: body(send.content),
    sentAtMs: send.sentAtMs,
    ...(send.correlationId !== undefined ? { correlationId: send.correlationId } : {}),
    ...(send.echo !== undefined ? { echo: { body: body(send.echo.content), timestampMs: send.echo.timestampMs } } : {}),
    ...(!matchedRowInStatus && send.matchedRow !== undefined ? { matchedRow: send.matchedRow } : {}),
    ...(matchedRowInStatus ? { matchedRowInStatus: true } : {}),
    ...(send.echoBoundary !== undefined ? { echoBoundary: send.echoBoundary } : {}),
    ...(send.droppedUploads !== undefined ? { droppedUploads: send.droppedUploads } : {}),
    ...(send.dismissed !== undefined ? { dismissed: send.dismissed } : {}),
    ...(status !== undefined ? { status } : {}),
    ...(send.requestFailed !== undefined ? { requestFailed: send.requestFailed } : {}),
    ...(send.requestUncertain !== undefined ? { requestUncertain: send.requestUncertain } : {}),
    ...(send.requestHadResponse !== undefined ? { requestHadResponse: send.requestHadResponse } : {}),
    ...(draft !== undefined ? { draft: {
      text: body(draft.text),
      attachments: draft.attachments,
      recordedVoiceAttachments: draft.recordedVoiceAttachments,
      capsules: draft.capsules as DurableDraft["capsules"],
      reply: draft.reply as DurableDraft["reply"],
    } } : {}),
  };
  return { format: "codeterm.chat-send", version: 1, tabId, sendId: send.id, revision, order, bodies, send: encoded };
}

export function encodeChatSendTombstone(tabId: string, sendId: string, revision = 0, order = 0): ChatSendDurableTombstone {
  return { format: "codeterm.chat-send", version: 1, tabId, sendId, revision, order, deleted: true, bodies: [] };
}

export function decodeChatSend(record: ChatSendDurableRecord): DeliverySend | null {
  if (!isDurableRecord(record) || record.deleted || !record.send) return null;
  try {
    const value = record.send;
    const content = readBody(record, value.content);
    if (content === null || value.id !== record.sendId || typeof value.sentAtMs !== "number") return null;
    const status = value.status ? decodeStatus(record.tabId, record, value.status) : undefined;
    const matchedRow = value.matchedRowInStatus ? status?.matched_row : value.matchedRow;
    const draftText = value.draft ? readBody(record, value.draft.text) : null;
    if (value.draft && draftText === null) return null;
    return {
      id: value.id,
      content,
      sentAtMs: value.sentAtMs,
      ...(value.correlationId !== undefined ? { correlationId: value.correlationId } : {}),
      ...(value.echo !== undefined ? { echo: { content: readRequiredBody(record, value.echo.body), timestampMs: value.echo.timestampMs } } : {}),
      ...(matchedRow !== undefined ? { matchedRow } : {}),
      ...(value.echoBoundary !== undefined ? { echoBoundary: value.echoBoundary } : {}),
      ...(value.droppedUploads !== undefined ? { droppedUploads: value.droppedUploads } : {}),
      ...(value.dismissed !== undefined ? { dismissed: value.dismissed } : {}),
      ...(status !== undefined ? { status } : {}),
      ...(value.requestFailed !== undefined ? { requestFailed: value.requestFailed } : {}),
      ...(value.requestUncertain !== undefined ? { requestUncertain: value.requestUncertain } : {}),
      ...(value.requestHadResponse !== undefined ? { requestHadResponse: value.requestHadResponse } : {}),
      ...(value.draft !== undefined ? { draft: {
        text: draftText!,
        attachments: value.draft.attachments,
        recordedVoiceAttachments: value.draft.recordedVoiceAttachments,
        capsules: value.draft.capsules,
        reply: value.draft.reply,
      } } : {}),
    };
  } catch {
    return null;
  }
}

export function mergeChatSendRecord(
  current: DeliverySend | undefined,
  previous: DeliverySend | undefined,
  next: DeliverySend,
): DeliverySend {
  if (!current) return next;
  const retry = previous != null && previous.correlationId === current.correlationId
    && previous.correlationId !== next.correlationId
    && deliveryState(current) === "failed"
    && !current.status?.system_send
    && !next.status?.system_send;
  let merged: DeliverySend = { ...current };
  if (retry) {
    merged = {
      ...merged,
      correlationId: next.correlationId,
      status: next.status,
      matchedRow: next.matchedRow,
      requestFailed: next.requestFailed,
      requestUncertain: next.requestUncertain,
      requestHadResponse: next.requestHadResponse,
    };
  }
  const patch = <K extends keyof DeliverySend>(key: K) => {
    if (retry && ["correlationId", "status", "matchedRow", "requestFailed", "requestUncertain", "requestHadResponse"].includes(key)) return;
    if (previous && sameValue(previous[key], next[key])) return;
    if (previous && !sameValue(current[key], previous[key])) return;
    (merged as DeliverySend)[key] = next[key] as DeliverySend[K];
  };
  for (const key of ["content", "sentAtMs", "echoBoundary", "droppedUploads", "requestFailed", "requestUncertain", "requestHadResponse", "draft"] as const) patch(key);
  if (!retry) patch("correlationId");
  if (!retry) patch("matchedRow");
  if (current.echo == null && next.echo != null && deliveryState(current) !== "failed"
    && (!previous || previous.echo == null || sameValue(current.echo, previous.echo))) merged.echo = next.echo;
  else if (previous?.echo != null && next.echo == null && sameValue(current.echo, previous.echo)) merged.echo = undefined;
  if (next.status) {
    const statusCorrelation = next.status.correlationId;
    if (statusCorrelation == null || statusCorrelation === merged.correlationId) merged = applyDeliveryStatus(merged, next.status);
  } else if (previous?.status && !sameValue(previous.status, next.status)
    && sameValue(current.status, previous.status) && current.status?.state === "failed") {
    merged.status = undefined;
  }
  if (next.dismissed === true || current.dismissed === true) merged.dismissed = true;
  return merged;
}

export function orderChatSendRecords(records: readonly ChatSendDurableRecord[]): ChatSendDurableRecord[] {
  return [...records].sort((left, right) => left.order - right.order);
}

export function sameDurableSend(left: DeliverySend, right: DeliverySend): boolean {
  const leftRecord = encodeChatSend("same", left);
  const rightRecord = encodeChatSend("same", right);
  return stableJson({ bodies: leftRecord.bodies, send: leftRecord.send })
    === stableJson({ bodies: rightRecord.bodies, send: rightRecord.send });
}

export function sameDurableRecord(left: ChatSendDurableRecord, right: ChatSendDurableRecord): boolean {
  const normalize = (record: ChatSendDurableRecord) => ({
    format: record.format,
    version: record.version,
    tabId: record.tabId,
    sendId: record.sendId,
    order: record.order,
    deleted: record.deleted,
    bodies: record.bodies,
    send: record.send,
  });
  return stableJson(normalize(left)) === stableJson(normalize(right));
}

export function durableRecordByteLength(record: ChatSendDurableRecord): number {
  return new TextEncoder().encode(JSON.stringify(record)).byteLength;
}

export function isDurableRecord(value: unknown): value is ChatSendDurableRecord {
  if (!value || typeof value !== "object") return false;
  const record = value as Partial<ChatSendDurableRecord>;
  return record.format === "codeterm.chat-send" && record.version === 1
    && typeof record.tabId === "string" && typeof record.sendId === "string"
    && Number.isSafeInteger(record.revision) && (record.revision ?? -1) >= 0
    && Number.isSafeInteger(record.order) && (record.order ?? -1) >= 0
    && Array.isArray(record.bodies) && record.bodies.every((item) => typeof item === "string")
    && (record.deleted === true
      ? record.send === undefined && record.bodies.length === 0
      : !!record.send && typeof record.send === "object");
}

function encodeStatus(status: ChatSendStatus, body: (value: string) => BodyRef): DurableStatus {
  const { tab_id: _tabId, system_send: systemSend, ...fields } = status;
  return {
    ...fields,
    ...(systemSend !== undefined ? { system_send: { body: body(systemSend.content), sent_at_ms: systemSend.sent_at_ms } } : {}),
  };
}

function decodeStatus(tabId: string, record: ChatSendDurableRecord, status: DurableStatus): ChatSendStatus {
  const { system_send: systemSend, ...fields } = status;
  return {
    ...fields,
    tab_id: tabId,
    ...(systemSend !== undefined ? { system_send: { content: readRequiredBody(record, systemSend.body), sent_at_ms: systemSend.sent_at_ms } } : {}),
  };
}

function readBody(record: ChatSendDurableRecord, ref: BodyRef): string | null {
  return Number.isSafeInteger(ref) && ref >= 0 && ref < record.bodies.length ? record.bodies[ref] : null;
}

function readRequiredBody(record: ChatSendDurableRecord, ref: BodyRef): string {
  const value = readBody(record, ref);
  if (value === null) throw new Error("Invalid chat send body reference");
  return value;
}

function stableJson(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(stableJson).join(",")}]`;
  if (value && typeof value === "object") {
    const entries = Object.entries(value as Record<string, unknown>)
      .filter(([, item]) => item !== undefined)
      .sort(([left], [right]) => left.localeCompare(right));
    return `{${entries.map(([key, item]) => `${JSON.stringify(key)}:${stableJson(item)}`).join(",")}}`;
  }
  return JSON.stringify(value) ?? "null";
}

function sameValue(left: unknown, right: unknown): boolean {
  return stableJson(left) === stableJson(right);
}
