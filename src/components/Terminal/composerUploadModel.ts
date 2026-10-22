import { VIDEO_SNIFF_BYTES, attachmentKindForUpload, attachmentKindForPath, validateAttachment } from "@codeterm/chat-core";
import { createAttachmentUploads, type AttachmentUpload } from "./attachmentUploadModel";
import { addCapsule, capsuleSpans, type CapsuleRegistry, type ComposerCapsule } from "./composerCapsules";
import { imageUploadName } from "./composerTransferModel";
import { prepareBrowserUpload } from "./browserImageUpload";

export interface ComposerUpload extends AttachmentUpload { label: string }

export function draftHasPendingUploads(text: string, registry: CapsuleRegistry): boolean {
  return capsuleSpans(text).some(({ label }) => registry[label]?.uploadId !== undefined);
}

/** Rejects a video the host would not play before any byte is stored; the chip shows the error. */
export async function validatedStore(store: (blob: Blob, name: string) => Promise<string>, blob: Blob, name: string): Promise<string> {
  const prepared = await prepareBrowserUpload(blob, name);
  blob = prepared.blob;
  name = prepared.name;
  const kind = attachmentKindForUpload({ name, mime: blob.type });
  const head = kind === "video" ? new Uint8Array(await blob.slice(0, VIDEO_SNIFF_BYTES).arrayBuffer()) : undefined;
  const verdict = validateAttachment({ kind, size: blob.size, head });
  if (!verdict.ok) throw new Error(verdict.error);
  return store(blob, imageUploadName(name, blob.type));
}

export function createComposerUploads(deps: {
  registry: () => CapsuleRegistry;
  commit: (registry: CapsuleRegistry) => void;
  insert: (capsule: ComposerCapsule) => void;
  /** The current draft text, so a capsule removed from it stops holding its number. */
  draft?: () => string | undefined;
  store: (blob: Blob, name: string) => Promise<string>;
  publish: (uploads: ComposerUpload[]) => void;
}) {
  const labels = new Map<number, string>();
  return createAttachmentUploads({
    store: (blob, name) => validatedStore(deps.store, blob, name),
    begin: (upload) => {
      const added = addCapsule(deps.registry(), {
        path: upload.name, kind: attachmentKindForUpload({ name: upload.name, mime: upload.blob.type }),
      }, deps.draft?.());
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
      deps.commit({ ...registry, [label]: { ...ready, path, kind: attachmentKindForPath(path) } });
    },
    publish: (uploads) => deps.publish(uploads.map((upload) => ({ ...upload, label: labels.get(upload.id)! }))),
  });
}
