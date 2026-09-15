import { classifyTabTarget, type TabTarget } from "../../lib/remoteTabTransport";
import { planComposerAttachments, type ComposerAttachmentPlan } from "./composerAttachmentPlan";

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
 * each attachment reference rewritten to its target-local copy. */
export async function composeAttachmentDraft(
  tabId: string,
  text: string,
  attachments: string[],
  recordedVoiceAttachments: string[],
  deps: AttachmentHandoffDeps,
): Promise<AttachmentDraft> {
  const target = attachmentHostTarget(tabId, deps.relayPeerId());
  const { parts, transcribeFailed } = await resolveAttachmentParts(
    target,
    planComposerAttachments(attachments, recordedVoiceAttachments),
    deps,
  );
  const pathLines = parts.join(" ");
  const composed = text && pathLines ? `${text} ${pathLines}` : text || pathLines;
  assertNoSourceAttachmentPaths(target, composed, attachments, deps.quote);
  return { composed, transcribeFailed };
}

async function resolveLocalParts(
  plans: ComposerAttachmentPlan[],
  deps: AttachmentHandoffDeps,
): Promise<AttachmentParts> {
  let transcribeFailed = false;
  const parts = await Promise.all(
    plans.map(async ({ path, transcribe }) => {
      if (!transcribe) return deps.quote(path);
      try {
        return await deps.transcribeLocal(path);
      } catch {
        transcribeFailed = true;
        return deps.quote(path);
      }
    }),
  );
  return { parts, transcribeFailed };
}

async function transferPart(
  target: { ordinaryTabId: string; peerId: string },
  plan: ComposerAttachmentPlan,
  deps: AttachmentHandoffDeps,
): Promise<string> {
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
  return kind === "voice" && transferred.reference ? transferred.reference : deps.quote(transferred.path);
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
