import { MAX_TEMP_FILE_DECODED_BYTES } from "../../../generated/config/attachmentLimits";
import type { AttachmentUpload } from "./attachmentUploadModel";

export { MAX_TEMP_FILE_DECODED_BYTES };
export type ComposerIssueAction = "retry" | "remove" | "send-path";
export interface ComposerIssue {
  id: number;
  kind: "too-large" | "upload";
  title: string;
  detail: string;
  explanation?: string;
  actions: ComposerIssueAction[];
  name: string;
}
export interface ComposerAttachment { uploadId: number; path: string }

const megabytes = (bytes: number) => `${Math.ceil(bytes / (1024 * 1024))} MB`;

export function composerIssues(attachments: readonly ComposerAttachment[], uploads: readonly AttachmentUpload[]): ComposerIssue[] {
  return uploads.filter(upload => upload.state === "failed").map(upload => {
    const message = upload.failure?.message ?? "";
    const tooLarge = upload.blob.size > MAX_TEMP_FILE_DECODED_BYTES || upload.failure?.status === 413 || /HTTP 413\b/.test(message);
    const voice = /^voice(?:-note)?[.-]/i.test(upload.name) || upload.blob.type.startsWith("audio/");
    const path = attachments.find(item => item.uploadId === upload.id)?.path;
    const actions: ComposerIssueAction[] = [];
    if (!tooLarge && upload.failure?.retryable) actions.push("retry");
    if (tooLarge && voice && path) actions.push("send-path");
    actions.push("remove");
    const reason = upload.failure?.status && upload.failure.status >= 500 ? "server unavailable"
      : upload.failure?.retryable ? "connection interrupted" : "could not upload attachment";
    return {
      id: upload.id, name: upload.name, kind: tooLarge ? "too-large" : "upload",
      title: tooLarge ? `${voice ? "Voice note" : upload.name} too large (${megabytes(upload.blob.size)}, limit ${megabytes(MAX_TEMP_FILE_DECODED_BYTES)})` : `Upload failed: ${reason}`,
      detail: message,
      explanation: tooLarge && voice && !path ? "This recording has no saved file path. Choose a shorter recording." : undefined,
      actions,
    };
  });
}
