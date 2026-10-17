import { isImagePath } from "@codeterm/chat-core";

export interface FileTransfer {
  files?: ArrayLike<File>;
  items?: ArrayLike<{ kind: string; type: string; getAsFile(): File | null }>;
  types?: readonly string[];
}

export function isFileTransfer(data: FileTransfer): boolean {
  return !!data.files?.length || Array.from(data.items ?? []).some(item => item.kind === "file")
    || Array.from(data.types ?? []).some(type => /^(Files|image\/.*|public\.(jpeg|png|heic|heif|image))$/i.test(type));
}

export function extractTransferFiles(data: FileTransfer): File[] {
  const files = [...Array.from(data.files ?? []), ...Array.from(data.items ?? [])
    .filter(item => item.kind === "file").map(item => item.getAsFile()).filter((file): file is File => file !== null)];
  return files.filter((file, index) => files.findIndex(other => other === file
    || (other.name === file.name && other.type === file.type && other.size === file.size && other.lastModified === file.lastModified)) === index);
}

export function planFileDrop(data: FileTransfer) {
  const files = extractTransferFiles(data);
  const unreadable = files.length === 0 && (isFileTransfer(data) || data.types?.includes("text/uri-list"));
  return { preventNavigation: true, files, error: unreadable ? "This drop contains no readable file. Use the file picker to attach the image." : null };
}

export function imageUploadName(name: string, mime: string): string {
  const type = mime.toLowerCase().split(";")[0];
  const extension = type === "public.jpeg" ? "jpg" : type === "public.png" ? "png" : undefined;
  const image = type.startsWith("image/") || /^public\.(jpeg|png|heic|heif|image)$/.test(type) || /\.(heic|heif)$/i.test(name);
  if (!image) return name;
  const suffix = extension ?? (type.startsWith("image/") ? type.slice(6).replace("jpeg", "jpg").replace("svg+xml", "svg") : "");
  if (!suffix || !isImagePath(`image.${suffix}`)) throw new Error(`Unsupported image format: ${mime || name}. Use PNG, JPEG or WebP.`);
  return isImagePath(name) ? name : `${name || "image"}.${suffix}`;
}
