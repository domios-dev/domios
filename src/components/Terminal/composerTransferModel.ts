import { authoredCause } from "../../lib/describeError";
import { attachmentKindForPath } from "@codeterm/chat-core";

export interface FileTransfer {
  files?: ArrayLike<File>;
  items?: ArrayLike<{ kind: string; type: string; getAsFile(): File | null }>;
  types?: readonly string[];
}

const IMAGE_UTI_EXTENSIONS = new Map([
  ["public.jpeg", "jpg"], ["public.png", "png"], ["public.heic", "heic"],
  ["public.heif", "heif"], ["com.compuserve.gif", "gif"], ["public.tiff", "tiff"],
  ["public.image", ""],
]);

function advertisesImage(type: string): boolean {
  const mime = type.toLowerCase();
  return mime.startsWith("image/") || IMAGE_UTI_EXTENSIONS.has(mime);
}

export function isFileTransfer(data: FileTransfer): boolean {
  return !!data.files?.length || Array.from(data.items ?? []).some(item => item.kind === "file" || advertisesImage(item.type))
    || Array.from(data.types ?? []).some(type => /^(files|public\.file-url)$/i.test(type) || advertisesImage(type));
}

export function acceptsFileDropPreview(data: FileTransfer): boolean {
  return !data.types?.length || Array.from(data.types).includes("text/uri-list");
}

function droppedFile(item: NonNullable<FileTransfer["items"]>[number]): File | null {
  if (item.kind !== "file" && !advertisesImage(item.type)) return null;
  try {
    return item.getAsFile();
  } catch {
    return null;
  }
}

// files and items describe the same payload; getAsFile() mints a new File per call, so mixing them duplicates pastes.
export function extractTransferFiles(data: FileTransfer): File[] {
  const listed = Array.from(data.files ?? []);
  if (listed.length) return listed;
  return Array.from(data.items ?? [], droppedFile).filter((file): file is File => file !== null);
}

const PLAIN_TEXT_UTI = /^public\.(?:utf8-)?plain-text$/i;

export function clipboardFileName(file: File, transferTypes: readonly string[] = []): string {
  const type = file.type.toLowerCase().split(";")[0];
  const textUti = PLAIN_TEXT_UTI.test(type) || transferTypes.some(value => PLAIN_TEXT_UTI.test(value));
  const name = file.name;
  if (PLAIN_TEXT_UTI.test(name?.replace(/\.txt$/i, "") ?? "")
    || (textUti && /^(?:blob|file|text)(?:\.txt)?$/i.test(name))) return "Pasted text.txt";
  if (name?.trim()) return name;
  return textUti || type === "text/plain" ? "Pasted text.txt" : "file.bin";
}

export function planFileDrop(data: FileTransfer) {
  const rawFiles = extractTransferFiles(data);
  const imageTypes = [
    ...Array.from(data.items ?? [], item => item.type),
    ...Array.from(data.types ?? []),
  ].filter(advertisesImage);
  const fallbackType = rawFiles.length === 1 ? imageTypes.find(type => imageUploadName("", type) !== "") : undefined;
  const files = rawFiles.map(file => {
    const name = imageUploadName(file.name, advertisesImage(file.type) ? file.type : fallbackType ?? file.type);
    return name === file.name ? file : new File([file], name, { type: file.type, lastModified: file.lastModified });
  }).filter(file => file.size > 0 || (attachmentKindForPath(file.name) !== "image" && !advertisesImage(file.type) && !imageTypes.length));
  const needsImageType = files.length === 1 && (imageTypes.length > 0 || advertisesImage(files[0].type))
    && attachmentKindForPath(files[0].name) !== "image";
  const unreadable = files.length === 0 && (isFileTransfer(data) || Array.from(data.types ?? []).includes("text/uri-list"));
  return { preventNavigation: true, files, needsImageType, error: unreadable ? unreadableFileDrop() : null };
}

export function unreadableFileDrop(): string {
  return authoredCause("This drop contains no readable file. Use the file picker to attach the image.");
}

export async function identifyOpaqueImage(file: File): Promise<File | null> {
  const bytes = new Uint8Array(await file.slice(0, 16).arrayBuffer());
  const ascii = String.fromCharCode(...bytes);
  let type: string | undefined;
  if (bytes[0] === 0xff && bytes[1] === 0xd8 && bytes[2] === 0xff) type = "image/jpeg";
  else if (ascii.startsWith("\x89PNG\r\n\x1a\n")) type = "image/png";
  else if (ascii.startsWith("GIF87a") || ascii.startsWith("GIF89a")) type = "image/gif";
  else if (ascii.startsWith("RIFF") && ascii.slice(8, 12) === "WEBP") type = "image/webp";
  else if (ascii.slice(4, 8) === "ftyp") {
    const brand = ascii.slice(8, 12);
    if (["heic", "heix", "hevc", "hevx"].includes(brand)) type = "image/heic";
    else if (["mif1", "msf1"].includes(brand)) type = "image/heif";
    else if (["avif", "avis"].includes(brand)) type = "image/avif";
  }
  return type ? new File([file], imageUploadName(file.name, type), { type, lastModified: file.lastModified }) : null;
}

export function imageUploadName(name: string, mime: string): string {
  const type = mime.toLowerCase().split(";")[0];
  const extension = IMAGE_UTI_EXTENSIONS.get(type);
  const suffix = extension ?? (type.startsWith("image/") ? type.slice(6).replace("jpeg", "jpg").replace("svg+xml", "svg") : "");
  if (!suffix || !/^[a-z0-9]+$/.test(suffix)) return name;
  return /\.(?:avif|bmp|gif|heic|heif|ico|jpe?g|png|svg|tiff?|webp)$/i.test(name)
    ? name : `${name.replace(/\.[a-z0-9]+$/i, "") || "image"}.${suffix}`;
}
