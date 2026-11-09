import type { AttachmentUpload } from "./attachmentUploadModel";
import { attachmentKindForPath } from "@codeterm/chat-core";
import { capsuleSpans, requireCapsulePaths, type CapsuleRegistry } from "./composerCapsules";

export function composerSendPlan(text: string, registry: CapsuleRegistry, uploads: readonly Pick<AttachmentUpload, "id" | "state">[]) {
  const uploadIds = capsuleSpans(text).flatMap(({ label }) => {
    const id = registry[label]?.uploadId;
    return id === undefined ? [] : [id];
  });
  return {
    clearComposer: true,
    uploadIds,
    waitForUploadIds: uploadIds.filter(id => uploads.some(upload => upload.id === id && upload.state === "uploading")),
    dropUploadIds: uploadIds.filter(id => uploads.some(upload => upload.id === id && upload.state === "failed")),
  } as const;
}

export function resolveComposerSend(text: string, registry: CapsuleRegistry, results: ReadonlyMap<number, string | null | undefined>) {
  requireCapsulePaths(text, registry, true);
  const capsules = { ...registry };
  let body = "";
  let cursor = 0;
  for (const span of capsuleSpans(text)) {
    body += text.slice(cursor, span.start);
    const capsule = registry[span.label];
    if (capsule?.uploadId !== undefined) {
      if (!results.has(capsule.uploadId)) throw new Error(`Upload result for attachment "${span.label}" is unavailable. Reattach it before sending.`);
      const path = results.get(capsule.uploadId);
      if (path) {
        const { uploadId: _id, ...ready } = capsule;
        capsules[span.label] = { ...ready, path, kind: attachmentKindForPath(path) };
        body += text.slice(span.start, span.end);
      } else delete capsules[span.label];
    } else body += text.slice(span.start, span.end);
    cursor = span.end;
  }
  return { text: (body + text.slice(cursor)).trim(), capsules };
}
