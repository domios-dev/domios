import { signalFrontendUploads } from "../../lib/frontendUpdate/appState";
import { useEffect, useRef, useState } from "react";
import { createAttachmentUploads, type AttachmentUpload } from "./attachmentUploadModel";
import { usePublishComposerIssues } from "./ComposerIssuesContext";
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
  usePublishComposerIssues(uploads, { retry: id => controller().retry(id), remove: id => controller().remove(id) });
  return {
    add: (blob: Blob, name: string) => controller().add(blob, name),
    store: async (blob: Blob, name: string) => (await controller().add(blob, name, false)) ?? "",
    addFiles: async (files: Iterable<File>) => { await Promise.all(Array.from(files, (file) => controller().add(file, file.name || "file.bin"))); },
    status: null,
    busy: uploads.some((upload) => upload.state === "uploading"),
  };
}
