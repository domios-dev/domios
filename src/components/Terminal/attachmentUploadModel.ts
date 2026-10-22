import { TransportError } from "../../lib/transportClient";
import { MAX_TEMP_FILE_DECODED_BYTES } from "../../../generated/config/attachmentLimits";

export interface UploadFailure {
  message: string;
  retryable: boolean;
  status?: number;
}

export function classifyUploadFailure(error: unknown): UploadFailure {
  const raw = error instanceof Error ? error.message : String(error);
  let message = raw;
  if (error instanceof TransportError) {
    try {
      const body: unknown = JSON.parse(error.body);
      const envelope = typeof body === "object" && body !== null ? body as Record<string, unknown> : {};
      const detail = typeof envelope.error === "object" && envelope.error !== null ? envelope.error as Record<string, unknown> : {};
      message = typeof detail.message === "string" ? detail.message
        : typeof envelope.error === "string" ? envelope.error
        : typeof envelope.message === "string" ? envelope.message : "The attachment could not be uploaded.";
    } catch { message = "The attachment could not be uploaded."; }
  }
  const status = typeof error === "object" && error !== null && "status" in error ? error.status : undefined;
  const interrupted = error instanceof TypeError || (error instanceof Error && ["AbortError", "NotReadableError"].includes(error.name));
  const multipart = status === 400 && /multipart\/form-data/i.test(raw);
  return { message, status: typeof status === "number" ? status : undefined, retryable: interrupted || multipart || status === 408 || status === 429 || (typeof status === "number" && status >= 500) };
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

export type UploadCardPresence = "hover" | "announce";

export function uploadCardPresence(hovered: boolean): UploadCardPresence {
  return hovered ? "hover" : "announce";
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
  const tasks = new Map<number, Promise<string | null | undefined>>();
  const detached = new Set<number>();
  const publish = () => { if (active) dependencies.publish(uploads.filter(item => !detached.has(item.id))); };
  async function run(upload: AttachmentUpload, attach = true) {
    try {
      if (upload.blob.size > MAX_TEMP_FILE_DECODED_BYTES) throw Object.assign(new Error("Attachment exceeds the upload size limit"), { status: 413 });
      const path = await dependencies.store(upload.blob, upload.name);
      if ((!active && !detached.has(upload.id)) || !uploads.some((item) => item.id === upload.id)) return;
      if (!path.trim()) throw new Error("Upload returned no attachment path");
      if (attach && !detached.has(upload.id)) await dependencies.attach([path], upload);
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
      const task = run(upload, attach);
      tasks.set(upload.id, task);
      void task.then(() => tasks.delete(upload.id));
      return task;
    },
    async retry(id: number) {
      const upload = uploads.find((item) => item.id === id);
      if (!upload || !canRetryUpload(upload)) return;
      const pending: AttachmentUpload = { ...upload, state: "uploading", failure: undefined };
      uploads = uploads.map((item) => item.id === id ? pending : item);
      publish();
      const task = run(pending);
      tasks.set(id, task);
      void task.then(() => tasks.delete(id));
      await task;
    },
    take(ids: number[]) {
      const captured = uploads.filter(item => ids.includes(item.id));
      for (const item of captured) detached.add(item.id);
      publish();
      return captured.map(upload => ({
        upload,
        result: (upload.state === "failed" ? Promise.resolve(null) : tasks.get(upload.id) ?? Promise.resolve(null))
          .then(path => {
            Object.assign(upload, uploads.find(item => item.id === upload.id));
            uploads = uploads.filter(item => item.id !== upload.id);
            tasks.delete(upload.id);
            detached.delete(upload.id);
            return path;
          }),
      }));
    },
    remove(id: number) { uploads = uploads.filter((item) => item.id !== id); publish(); },
    dispose() { active = false; uploads = uploads.filter(item => detached.has(item.id)); },
  };
}
