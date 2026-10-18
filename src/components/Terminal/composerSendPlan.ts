import type { AttachmentUpload } from "./attachmentUploadModel";
import { attachmentKindForPath } from "@codeterm/chat-core";
import { capsuleSpans, type CapsuleRegistry } from "./composerCapsules";

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
  const capsules = { ...registry };
  let body = "";
  let cursor = 0;
  for (const span of capsuleSpans(text)) {
    body += text.slice(cursor, span.start);
    const capsule = registry[span.label];
    if (capsule?.uploadId !== undefined) {
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
