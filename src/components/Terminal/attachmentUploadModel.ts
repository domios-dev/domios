export interface UploadFailure {
  message: string;
  retryable: boolean;
}

export function classifyUploadFailure(error: unknown): UploadFailure {
  const message = error instanceof Error ? error.message : String(error);
  const status = typeof error === "object" && error !== null && "status" in error ? error.status : undefined;
  const interrupted = error instanceof TypeError || (error instanceof Error && ["AbortError", "NotReadableError"].includes(error.name));
  const multipart = status === 400 && /multipart\/form-data/i.test(message);
  return { message, retryable: interrupted || multipart || status === 408 || status === 429 || (typeof status === "number" && status >= 500) };
}

export interface AttachmentUpload {
  id: number;
  name: string;
  blob: Blob;
  state: "uploading" | "failed";
  failure?: UploadFailure;
}

export function canRetryUpload(upload: AttachmentUpload): boolean {
  return upload.state === "failed" && upload.failure?.retryable === true;
}

export function createAttachmentUploads(dependencies: {
  store: (blob: Blob, name: string) => Promise<string>;
  attach: (paths: string[], upload: AttachmentUpload) => unknown;
  begin?: (upload: AttachmentUpload) => void;
  publish: (uploads: AttachmentUpload[]) => void;
}) {
  let sequence = 0;
  let uploads: AttachmentUpload[] = [];
  let active = true;
  const publish = () => { if (active) dependencies.publish([...uploads]); };
  async function run(upload: AttachmentUpload, attach = true) {
    try {
      const bytes = await upload.blob.arrayBuffer();
      if (bytes.byteLength !== upload.blob.size) throw new TypeError("Attachment bytes are unavailable; retry the upload");
      const snapshot = new Blob([bytes], { type: upload.blob.type });
      uploads = uploads.map((item) => item.id === upload.id ? { ...item, blob: snapshot } : item);
      const path = await dependencies.store(snapshot, upload.name);
      if (!active || !uploads.some((item) => item.id === upload.id)) return;
      if (!path.trim()) throw new Error("Upload returned no attachment path");
      if (attach) await dependencies.attach([path], upload);
      uploads = uploads.filter((item) => item.id !== upload.id);
      publish();
      return path;
    } catch (error) {
      uploads = uploads.map((item) => item.id === upload.id ? { ...item, state: "failed", failure: classifyUploadFailure(error) } : item);
    }
    publish();
    return null;
  }
  return {
    async add(blob: Blob, name: string, attach = true) {
      const upload: AttachmentUpload = { id: ++sequence, name, blob, state: "uploading" };
      uploads = [...uploads, upload];
      dependencies.begin?.(upload);
      publish();
      return run(upload, attach);
    },
    async retry(id: number) {
      const upload = uploads.find((item) => item.id === id);
      if (!upload || !canRetryUpload(upload)) return;
      const pending: AttachmentUpload = { ...upload, state: "uploading", failure: undefined };
      uploads = uploads.map((item) => item.id === id ? pending : item);
      publish();
      await run(pending);
    },
    remove(id: number) { uploads = uploads.filter((item) => item.id !== id); publish(); },
    dispose() { active = false; uploads = []; },
  };
}
