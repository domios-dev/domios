import { AttachmentUploadNotice } from "./AttachmentUploadNotice";
import { capsuleSpans, type CapsuleRegistry, type CapsuleSpan } from "./composerCapsules";
import type { ComposerUpload } from "./composerUploadModel";

export function ComposerUploadNotices({ value, registry, uploads, onRemove }: {
  value: string; registry: CapsuleRegistry; uploads: ComposerUpload[]; onRemove: (span: CapsuleSpan) => void;
}) {
  return <div className="flex flex-col gap-1" aria-live="polite">{uploads.filter(upload => upload.state === "failed").map(upload => {
    const span = capsuleSpans(value).find(span => registry[span.label]?.uploadId === upload.id);
    return span ? <AttachmentUploadNotice key={upload.id} upload={upload} onRemove={() => onRemove(span)} /> : null;
  })}</div>;
}
