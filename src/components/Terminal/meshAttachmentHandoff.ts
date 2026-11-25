import {
  attachmentMarkerSegments,
  formatAttachmentMarker,
  hasAttachmentMarkers,
  parseMarker,
  visibleAttachmentLabel,
  type AttachmentMarker,
} from "@codeterm/chat-core";
import { classifyTabTarget, type TabTarget } from "../../lib/remoteTabTransport";
import { planComposerAttachments, type ComposerAttachmentPlan } from "./composerAttachmentPlan";
import { inlineReplyText, replyMarker, withInlineReply, withReplyMarker, type ComposerReply } from "./composerReply";
import {
  capsuleMarker,
  capsulesInDraft,
  EMPTY_CAPSULES,
  serializeCapsuleDraft,
  requireCapsulePaths,
  type CapsuleRegistry,
  type ComposerCapsule,
} from "./composerCapsules";

export type MeshAttachmentKind = import("../../../gen/MeshAttachmentKind").MeshAttachmentKind;

export type MeshAttachmentTransferRequest = Record<string, unknown> & {ordinaryTabId: import("../../../gen/MeshAttachmentTransferBody").MeshAttachmentTransferBody["ordinary_tab_id"]; peerId: import("../../../gen/MeshAttachmentTransferBody").MeshAttachmentTransferBody["peer_id"]; path: import("../../../gen/MeshAttachmentTransferBody").MeshAttachmentTransferBody["path"]; kind: import("../../../gen/MeshAttachmentTransferBody").MeshAttachmentTransferBody["kind"]};

export type TransferredAttachment = Omit<import("../../../gen/MeshIngestedAttachment").MeshIngestedAttachment, "reference"> & Partial<Pick<import("../../../gen/MeshIngestedAttachment").MeshIngestedAttachment, "reference">>;

export interface AttachmentHandoffDeps {
  /** Peer the web mirror relays tab traffic to; its tab ids are peer-local. */
  relayPeerId: () => string | null;
  transfer: (request: MeshAttachmentTransferRequest) => Promise<TransferredAttachment>;
  transcribeLocal: (path: string) => Promise<string>;
  /** Local path classification on the viewer host for raw inline attachment markers. */
  localPathKinds?: (paths: string[]) => Promise<string[]>;
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

/** Optimistic bubble text for a remote send before transfer: keeps text and labels, never viewer-local paths. */
export function remoteOptimisticContent(
  text: string,
  capsules: CapsuleRegistry,
  attachments: string[],
  reply: ComposerReply | null,
  inlineReply = false,
): string {
  const withCapsules = serializeCapsuleDraft(text, capsules, (capsule) =>
    capsule.displayName ?? visibleAttachmentLabel(capsule.label));
  const withoutMarkers = attachmentMarkerSegments(withCapsules)
    .map((segment) => ("text" in segment ? segment.text
      : segment.attachment.label ? visibleAttachmentLabel(segment.attachment.label) : attachmentName(segment.attachment.path)))
    .join("");
  const extra = attachments.map(attachmentName).join(" ");
  const replyPart = reply ? (inlineReply || !reply.path ? inlineReplyText(reply) : reply.label) : "";
  return [replyPart, withoutMarkers.trim(), extra].filter(Boolean).join(" ").trim();
}

/** An attachment belongs to the target tab's host: a remote target gets a
 * target-local reference or the send is refused before delivery. */
export async function resolveAttachmentParts(
  target: TabTarget,
  plans: ComposerAttachmentPlan[],
  deps: AttachmentHandoffDeps,
  transferCache?: Map<string, Promise<TransferredAttachment>>,
): Promise<AttachmentParts> {
  if (target.kind === "invalid-remote") {
    throw new AttachmentTransferError(`Invalid remote tab id "${target.value}"; attachments were not sent`);
  }
  if (target.kind === "local") return resolveLocalParts(plans, deps);
  const parts = await Promise.all(plans.map((plan) => transferPart(target, plan, deps, transferCache)));
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
  inlineReply = false,
): Promise<AttachmentDraft> {
  requireCapsulePaths(text, capsules);
  const target = attachmentHostTarget(tabId, deps.relayPeerId());
  const transferCache = new Map<string, Promise<TransferredAttachment>>();
  const inlineMarkers = await resolveInlineAttachmentMarkers(target, text, deps, transferCache);
  const inline = capsulesInDraft(inlineMarkers.text, capsules);
  const markers = await resolveCapsuleMarkers(target, inline, deps, transferCache);
  const typed = serializeCapsuleDraft(inlineMarkers.text, capsules, (capsule) => markers.get(capsule.label) ?? capsuleMarker(capsule));
  const useInlineReply = Boolean(reply && (inlineReply || !reply.path));
  const body = reply
    ? (useInlineReply ? withInlineReply(inlineReplyText(reply), typed) : withReplyMarker(await resolveReplyMarker(target, reply, deps, transferCache), typed))
    : typed;
  const { parts, transcribeFailed } = await resolveAttachmentParts(
    target,
    planComposerAttachments(attachments, recordedVoiceAttachments),
    deps,
    transferCache,
  );
  const pathLines = parts.join(" ");
  const composed = body && pathLines ? `${body} ${pathLines}` : body || pathLines;
  const sourcePaths = [
    ...attachments,
    ...inline.map((capsule) => capsule.path),
    ...inlineMarkers.transferredSources,
    ...(reply && !useInlineReply ? [reply.path] : []),
  ];
  assertNoSourceAttachmentPaths(target, composed, sourcePaths, deps.quote);
  return { composed, transcribeFailed };
}

async function resolveInlineAttachmentMarkers(
  target: TabTarget,
  text: string,
  deps: AttachmentHandoffDeps,
  transferCache: Map<string, Promise<TransferredAttachment>>,
): Promise<{ text: string; transferredSources: string[] }> {
  if (target.kind === "local" || !hasAttachmentMarkers(text)) return { text, transferredSources: [] };
  if (target.kind === "invalid-remote") {
    throw new AttachmentTransferError(`Invalid remote tab id "${target.value}"; attachments were not sent`);
  }
  const segments = attachmentMarkerSegments(text);
  const uniquePaths = [...new Set(segments.flatMap((segment) => ("attachment" in segment ? [segment.attachment.path] : [])))];
  if (!uniquePaths.length) return { text, transferredSources: [] };
  let kindsByPath: Map<string, string> | null = null;
  if (deps.localPathKinds) {
    try {
      const kinds = await deps.localPathKinds(uniquePaths);
      if (Array.isArray(kinds) && kinds.length === uniquePaths.length) {
        kindsByPath = new Map(uniquePaths.map((path, index) => [path, kinds[index]]));
      }
    } catch {
      // Fail closed to transferring rather than leaking an unchecked viewer-local path.
      kindsByPath = null;
    }
  }
  const transferredSources: string[] = [];
  const rewritten = await Promise.all(segments.map(async (segment) => {
    if ("text" in segment) return segment.text;
    const attachment = segment.attachment;
    const localKind = kindsByPath?.get(attachment.path);
    if (localKind === "missing") return formatInlineAttachment(attachment, attachment.path);
    if (localKind === "folder" || (!kindsByPath && attachment.kind === "folder")) {
      const label = attachment.label ?? attachmentName(attachment.path);
      throw new AttachmentTransferError(`${label} is a folder on this machine; folders cannot be sent to a remote tab`);
    }
    const transcribe = attachment.kind === "voice";
    const copy = await transferCopy(target, { path: attachment.path, transcribe }, deps, transferCache);
    transferredSources.push(attachment.path);
    return transcribe ? requireVoiceTranscript(copy.reference) : formatInlineAttachment(attachment, copy.path);
  }));
  return { text: rewritten.join(""), transferredSources };
}

function formatInlineAttachment(attachment: AttachmentMarker, path: string): string {
  const reply = attachment.reply !== undefined ? { ref: attachment.reply, excerpt: attachment.excerpt ?? "" } : undefined;
  return formatAttachmentMarker(path, attachment.kind, attachment.label, reply);
}

async function resolveReplyMarker(
  target: TabTarget,
  reply: ComposerReply,
  deps: AttachmentHandoffDeps,
  transferCache: Map<string, Promise<TransferredAttachment>>,
): Promise<string> {
  if (target.kind === "invalid-remote") {
    throw new AttachmentTransferError(`Invalid remote tab id "${target.value}"; attachments were not sent`);
  }
  if (target.kind !== "remote") return replyMarker(reply);
  const copy = await transferCopy(target, { path: reply.path, transcribe: false }, deps, transferCache);
  return replyMarker(reply, copy.path);
}

async function resolveCapsuleMarkers(
  target: TabTarget,
  capsules: ComposerCapsule[],
  deps: AttachmentHandoffDeps,
  transferCache: Map<string, Promise<TransferredAttachment>>,
): Promise<Map<string, string>> {
  if (target.kind === "invalid-remote" && capsules.length) {
    throw new AttachmentTransferError(`Invalid remote tab id "${target.value}"; attachments were not sent`);
  }
  if (target.kind !== "remote") return new Map(capsules.map((capsule) => [capsule.label, capsuleMarker(capsule)]));
  const entries = await Promise.all(capsules.map(async (capsule): Promise<[string, string]> => {
    if (capsule.kind === "folder") {
      throw new AttachmentTransferError(`${capsule.label} is a folder on this machine; folders cannot be sent to a remote tab`);
    }
    const copy = await transferCopy(target, { path: capsule.path, transcribe: false }, deps, transferCache);
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
  transferCache?: Map<string, Promise<TransferredAttachment>>,
): Promise<TransferredAttachment> {
  const kind: MeshAttachmentKind = plan.transcribe ? "voice" : "file";
  const cacheKey = `${kind}:${plan.path}`;
  const cached = transferCache?.get(cacheKey);
  if (cached) return cached;
  const run = (async () => {
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
  })();
  transferCache?.set(cacheKey, run);
  return run;
}

async function transferPart(
  target: { ordinaryTabId: string; peerId: string },
  plan: ComposerAttachmentPlan,
  deps: AttachmentHandoffDeps,
  transferCache?: Map<string, Promise<TransferredAttachment>>,
): Promise<string> {
  const transferred = await transferCopy(target, plan, deps, transferCache);
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
  if (!reference?.trim()) throw new Error("Voice transcription failed: The target host returned no voice transcript");
  const marker = parseMarker(reference);
  if (marker?.kind === "voice" && marker.attrs.status === "untranscribed") {
    throw new Error(`Voice transcription failed: ${marker.payload.trim() || "The target host could not transcribe the voice note"}`);
  }
  return reference;
}
