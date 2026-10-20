import type { AttachmentUpload } from "./attachmentUploadModel";

export function AttachmentUploadNotice({ upload, onRetry, onRemove }: {
  upload: AttachmentUpload;
  onRetry?: () => void;
  onRemove?: () => void;
}) {
  return <span role="note" className="text-xs text-[var(--ct-muted-fg)]">
    {upload.name} was not attached.
    {onRetry && <button type="button" aria-label={`Retry attachment ${upload.name}`} onClick={onRetry}>Retry</button>}
    {onRemove && <button type="button" aria-label={`Remove attachment ${upload.name}`} onClick={onRemove}>Remove</button>}
  </span>;
}
