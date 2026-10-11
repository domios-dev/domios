import { signalFrontendUploads } from "../../lib/frontendUpdate/appState";
import { useEffect, useRef, useState } from "react";
import { canRetryUpload, createAttachmentUploads, type AttachmentUpload } from "./attachmentUploadModel";
import { storeBlob } from "./voice/audio";

export function useAttachmentUploads(onAttach: (paths: string[]) => unknown) {
  const attachRef = useRef(onAttach);
  attachRef.current = onAttach;
  const [uploads, setUploads] = useState<AttachmentUpload[]>([]);
  const uploadOwner = useRef({}).current;
  const controllerRef = useRef<ReturnType<typeof createAttachmentUploads> | null>(null);
  const controller = () => controllerRef.current ??= createAttachmentUploads({
    store: storeBlob, attach: (paths) => attachRef.current(paths), publish: next => { signalFrontendUploads(uploadOwner, next.filter(item => item.state === "uploading").length); setUploads(next); },
  });
  useEffect(() => () => { controllerRef.current?.dispose(); controllerRef.current = null; signalFrontendUploads(uploadOwner, 0); }, []);
  return {
    add: (blob: Blob, name: string) => controller().add(blob, name),
    store: async (blob: Blob, name: string) => (await controller().add(blob, name, false)) ?? "",
    addFiles: async (files: Iterable<File>) => { await Promise.all(Array.from(files, (file) => controller().add(file, file.name || "file.bin"))); },
    status: <div role="status" className="text-xs text-[var(--ct-muted-fg)]">
      {uploads.map((upload) => <div key={upload.id} className="flex items-center gap-2">
        <span>{upload.name}: {upload.state === "uploading" ? "Uploading…" : upload.failure?.message}</span>
        {canRetryUpload(upload) && <button type="button" aria-label={`Retry attachment ${upload.name}`} onClick={() => void controller().retry(upload.id)}>Retry</button>}
        <button type="button" aria-label={`Remove attachment ${upload.name}`} onClick={() => controller().remove(upload.id)}>Remove</button>
      </div>)}
    </div>,
    busy: uploads.some((upload) => upload.state === "uploading"),
  };
}
