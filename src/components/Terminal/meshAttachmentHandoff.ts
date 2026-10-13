import { formatAttachmentMarker, parseMarker } from "@codeterm/chat-core";
import { classifyTabTarget, type TabTarget } from "../../lib/remoteTabTransport";
import { planComposerAttachments, type ComposerAttachmentPlan } from "./composerAttachmentPlan";
import { replyMarker, withReplyMarker, type ComposerReply } from "./composerReply";
import {
  capsuleMarker,
  capsulesInDraft,
  EMPTY_CAPSULES,
  serializeCapsuleDraft,
  type CapsuleRegistry,
  type ComposerCapsule,
} from "./composerCapsules";

export type MeshAttachmentKind = "voice" | "file";

export interface MeshAttachmentTransferRequest extends Record<string, unknown> {
  ordinaryTabId: string;
  peerId: string;
  path: string;
  kind: MeshAttachmentKind;
}

export interface TransferredAttachment {
  path: string;
  kind: MeshAttachmentKind;
  size: number;
  reference?: string | null;
}

export interface AttachmentHandoffDeps {
  /** Peer the web mirror relays tab traffic to; its tab ids are peer-local. */
  relayPeerId: () => string | null;
  transfer: (request: MeshAttachmentTransferRequest) => Promise<TransferredAttachment>;
  transcribeLocal: (path: string) => Promise<string>;
  quote: (path: string) => string;
}

export class AttachmentTransferError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "AttachmentTransferError";
  }
}

export interface AttachmentParts {
  parts: string[];
  transcribeFailed: boolean;
}

export interface AttachmentDraft {
  composed: string;
  transcribeFailed: boolean;
}

function attachmentName(path: string): string {
  return path.split(/[\\/]/).pop() || "attachment";
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** The host that owns a tab: a composite mesh id names its peer, and a relayed
 * web mirror addresses the relay peer's own tabs. Bytes stay on this host. */
export function attachmentHostTarget(tabId: string, relayPeerId: string | null): TabTarget {
  const target = classifyTabTarget(tabId);
  if (target.kind !== "local" || !relayPeerId) return target;
  return classifyTabTarget(`mesh:${relayPeerId}/${tabId}`);
}

/** An attachment belongs to the target tab's host: a remote target gets a
 * target-local reference or the send is refused before delivery. */
export async function resolveAttachmentParts(
  target: TabTarget,
  plans: ComposerAttachmentPlan[],
  deps: AttachmentHandoffDeps,
): Promise<AttachmentParts> {
  if (target.kind === "invalid-remote") {
    throw new AttachmentTransferError(`Invalid remote tab id "${target.value}"; attachments were not sent`);
  }
  if (target.kind === "local") return resolveLocalParts(plans, deps);
  const parts = await Promise.all(plans.map((plan) => transferPart(target, plan, deps)));
  assertNoSourceAttachmentPaths(target, parts.join(" "), plans.map((plan) => plan.path), deps.quote);
  return { parts, transcribeFailed: false };
}

/** Message text for a composer draft; the only change a remote target sees is
 * each attachment reference rewritten to its target-local copy. Inline capsules
 * become tags where they were typed; a reply leads the text as its quote tag. */
export async function composeAttachmentDraft(
  tabId: string,
  text: string,
  attachments: string[],
  recordedVoiceAttachments: string[],
  deps: AttachmentHandoffDeps,
  capsules: CapsuleRegistry = EMPTY_CAPSULES,
  reply: ComposerReply | null = null,
): Promise<AttachmentDraft> {
  const target = attachmentHostTarget(tabId, deps.relayPeerId());
  const inline = capsulesInDraft(text, capsules);
  const markers = await resolveCapsuleMarkers(target, inline, deps);
  const typed = serializeCapsuleDraft(text, capsules, (capsule) => markers.get(capsule.label) ?? capsuleMarker(capsule));
  const body = reply ? withReplyMarker(await resolveReplyMarker(target, reply, deps), typed) : typed;
  const { parts, transcribeFailed } = await resolveAttachmentParts(
    target,
    planComposerAttachments(attachments, recordedVoiceAttachments),
    deps,
  );
  const pathLines = parts.join(" ");
  const composed = body && pathLines ? `${body} ${pathLines}` : body || pathLines;
  const sourcePaths = [...attachments, ...inline.map((capsule) => capsule.path), ...(reply ? [reply.path] : [])];
  assertNoSourceAttachmentPaths(target, composed, sourcePaths, deps.quote);
  return { composed, transcribeFailed };
}

async function resolveReplyMarker(target: TabTarget, reply: ComposerReply, deps: AttachmentHandoffDeps): Promise<string> {
  if (target.kind === "invalid-remote") {
    throw new AttachmentTransferError(`Invalid remote tab id "${target.value}"; attachments were not sent`);
  }
  if (target.kind !== "remote") return replyMarker(reply);
  const copy = await transferCopy(target, { path: reply.path, transcribe: false }, deps);
  return replyMarker(reply, copy.path);
}

async function resolveCapsuleMarkers(
  target: TabTarget,
  capsules: ComposerCapsule[],
  deps: AttachmentHandoffDeps,
): Promise<Map<string, string>> {
  if (target.kind === "invalid-remote" && capsules.length) {
    throw new AttachmentTransferError(`Invalid remote tab id "${target.value}"; attachments were not sent`);
  }
  if (target.kind !== "remote") return new Map(capsules.map((capsule) => [capsule.label, capsuleMarker(capsule)]));
  const entries = await Promise.all(capsules.map(async (capsule): Promise<[string, string]> => {
    if (capsule.kind === "folder") {
      throw new AttachmentTransferError(`${capsule.label} is a folder on this machine; folders cannot be sent to a remote tab`);
    }
    const copy = await transferCopy(target, { path: capsule.path, transcribe: false }, deps);
    return [capsule.label, capsuleMarker({ ...capsule, path: copy.path })];
  }));
  return new Map(entries);
}

async function resolveLocalParts(
  plans: ComposerAttachmentPlan[],
  deps: AttachmentHandoffDeps,
): Promise<AttachmentParts> {
  const parts = await Promise.all(plans.map(async ({ path, transcribe }) => {
    if (!transcribe) return formatAttachmentMarker(path);
    try {
      return requireVoiceTranscript(await deps.transcribeLocal(path));
    } catch (error) {
      throw new Error(`Voice transcription failed: ${errorMessage(error)}`);
    }
  }));
  return { parts, transcribeFailed: false };
}

async function transferCopy(
  target: { ordinaryTabId: string; peerId: string },
  plan: ComposerAttachmentPlan,
  deps: AttachmentHandoffDeps,
): Promise<TransferredAttachment> {
  const kind: MeshAttachmentKind = plan.transcribe ? "voice" : "file";
  const name = attachmentName(plan.path);
  let transferred: TransferredAttachment;
  try {
    transferred = await deps.transfer({
      ordinaryTabId: target.ordinaryTabId,
      peerId: target.peerId,
      path: plan.path,
      kind,
    });
  } catch (error) {
    throw new AttachmentTransferError(`${name} was not sent to the remote tab: ${errorMessage(error)}`);
  }
  if (!transferred?.path || transferred.path === plan.path) {
    throw new AttachmentTransferError(`${name} was not sent to the remote tab: the target host kept no copy`);
  }
  return transferred;
}

async function transferPart(
  target: { ordinaryTabId: string; peerId: string },
  plan: ComposerAttachmentPlan,
  deps: AttachmentHandoffDeps,
): Promise<string> {
  const transferred = await transferCopy(target, plan, deps);
  return plan.transcribe ? requireVoiceTranscript(transferred.reference) : formatAttachmentMarker(transferred.path);
}

export function assertNoSourceAttachmentPaths(
  target: TabTarget,
  text: string,
  sourcePaths: string[],
  quote: (path: string) => string,
): void {
  if (target.kind === "local") return;
  for (const source of sourcePaths) {
    if (!source) continue;
    if (text.includes(source) || text.includes(quote(source))) {
      throw new AttachmentTransferError(
        `${attachmentName(source)} still points at this machine; the message was not sent`,
      );
    }
  }
}

function requireVoiceTranscript(reference: string | null | undefined): string {
  if (!reference?.trim()) throw new Error("The target host returned no voice transcript");
  const marker = parseMarker(reference);
  if (marker?.kind === "voice" && marker.attrs.status === "untranscribed") {
    throw new Error(marker.payload.trim() || "The target host could not transcribe the voice note");
  }
  return reference;
}
