import { classifyTabTarget } from "../../lib/remoteTabTransport";
import type { ComposerAttachmentPlan } from "./composerAttachmentPlan";

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

function attachmentName(path: string): string {
  return path.split(/[\\/]/).pop() || "attachment";
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** An attachment belongs to the target tab's host: a remote target gets a
 * target-local reference or the send is refused before delivery. */
export async function resolveAttachmentParts(
  tabId: string,
  plans: ComposerAttachmentPlan[],
  deps: AttachmentHandoffDeps,
): Promise<AttachmentParts> {
  const target = classifyTabTarget(tabId);
  if (target.kind === "invalid-remote") {
    throw new AttachmentTransferError(`Invalid remote tab id "${target.value}"; attachments were not sent`);
  }
  if (target.kind === "local") return resolveLocalParts(plans, deps);
  const parts = await Promise.all(plans.map((plan) => transferPart(target, plan, deps)));
  assertNoSourceAttachmentPaths(tabId, parts.join(" "), plans.map((plan) => plan.path), deps.quote);
  return { parts, transcribeFailed: false };
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
  tabId: string,
  text: string,
  sourcePaths: string[],
  quote: (path: string) => string,
): void {
  if (classifyTabTarget(tabId).kind === "local") return;
  for (const source of sourcePaths) {
    if (!source) continue;
    if (text.includes(source) || text.includes(quote(source))) {
      throw new AttachmentTransferError(
        `${attachmentName(source)} still points at this machine; the message was not sent`,
      );
    }
  }
}
