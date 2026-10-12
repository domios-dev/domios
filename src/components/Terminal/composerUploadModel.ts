import { attachmentKindForPath } from "@codeterm/chat-core";
import { createAttachmentUploads, type AttachmentUpload } from "./attachmentUploadModel";
import { addCapsule, capsuleSpans, type CapsuleRegistry, type ComposerCapsule } from "./composerCapsules";

export interface ComposerUpload extends AttachmentUpload { label: string }

export function draftHasPendingUploads(text: string, registry: CapsuleRegistry): boolean {
  return capsuleSpans(text).some(({ label }) => registry[label]?.uploadId !== undefined);
}

export function createComposerUploads(deps: {
  registry: () => CapsuleRegistry;
  commit: (registry: CapsuleRegistry) => void;
  insert: (capsule: ComposerCapsule) => void;
  store: (blob: Blob, name: string) => Promise<string>;
  publish: (uploads: ComposerUpload[]) => void;
}) {
  const labels = new Map<number, string>();
  return createAttachmentUploads({
    store: deps.store,
    begin: (upload) => {
      const added = addCapsule(deps.registry(), {
        path: upload.name, kind: upload.blob.type.startsWith("image/") ? "image" : attachmentKindForPath(upload.name),
      });
      const capsule = { ...added.capsule, path: "", uploadId: upload.id };
      labels.set(upload.id, capsule.label);
      deps.commit({ ...added.registry, [capsule.label]: capsule });
      deps.insert(capsule);
    },
    attach: ([path], upload) => {
      const label = labels.get(upload.id);
      if (!label) return;
      const registry = deps.registry();
      const capsule = registry[label];
      if (capsule?.uploadId !== upload.id) return;
      const { uploadId: _uploadId, ...ready } = capsule;
      deps.commit({ ...registry, [label]: { ...ready, path } });
    },
    publish: (uploads) => deps.publish(uploads.map((upload) => ({ ...upload, label: labels.get(upload.id)! }))),
  });
}
