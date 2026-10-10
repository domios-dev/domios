export interface CodexUserContentItem {
  type?: string;
  text?: string;
  image_url?: string;
}

export interface CodexUserImage {
  name: string | null;
  path: string | null;
  mediaType: string;
  data: string | null;
}

export interface CodexUserContent {
  text: string;
  images: CodexUserImage[];
}

const INLINE_DATA_LIMIT = 100_000;
const OPEN_TAG = /^<image\s+name=(\[Image #\d+\])(?:\s+path="([^"]*)")?\s*>$/;
const INLINE_IMAGE = /<image\s+name=(\[Image #\d+\])(?:\s+path="([^"]*)")?\s*>[\s\S]*?<\/image>/g;
const CLOSE_TAG = /^<\/image>$/;
const PLACEHOLDER = /\[Image #\d+\]/g;

const EXT_MEDIA: Record<string, string> = {
  png: "image/png",
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  gif: "image/gif",
  webp: "image/webp",
  bmp: "image/bmp",
  svg: "image/svg+xml",
};

function mediaTypeFromPath(path: string | null): string {
  const ext = path?.split(".").pop()?.toLowerCase() ?? "";
  return EXT_MEDIA[ext] ?? "image/png";
}

function imageFromTag(name: string, rawPath: string | undefined): CodexUserImage {
  const path = rawPath?.trim() || null;
  return { name, path, mediaType: mediaTypeFromPath(path), data: null };
}

function withDataUrl(image: CodexUserImage, url: string | undefined): CodexUserImage {
  const match = /^data:([^;,]+);base64,([\s\S]*)$/.exec(url ?? "");
  if (!match) return image;
  const inline = !image.path && match[2].length < INLINE_DATA_LIMIT ? match[2] : null;
  return { ...image, mediaType: match[1], data: inline };
}

function extractInlineImages(text: string, images: CodexUserImage[]): string {
  return text.replace(INLINE_IMAGE, (_whole, name: string, path: string | undefined) => {
    images.push(imageFromTag(name, path));
    return "";
  });
}

function stripPlaceholders(text: string, images: CodexUserImage[]): string {
  if (images.length === 0) return text;
  const names = new Set(images.map((image) => image.name).filter(Boolean));
  return text
    .replace(PLACEHOLDER, (token) => (names.has(token) ? "" : token))
    .split("\n")
    .map((line) => line.replace(/[ \t]{2,}/g, " ").trim())
    .join("\n")
    .trim();
}

/** Codex records a pasted image as `<image name=[Image #N] path="...">`, an
 *  `input_image`, `</image>`, then the composer text carrying `[Image #N]`. */
export function parseCodexUserContent(content: string | CodexUserContentItem[] | undefined): CodexUserContent {
  const images: CodexUserImage[] = [];
  if (typeof content === "string") {
    return { text: stripPlaceholders(extractInlineImages(content, images), images), images };
  }
  let text = "";
  let open: CodexUserImage | null = null;
  for (const item of content ?? []) {
    if (!item) continue;
    if (item.type === "input_image") {
      if (open) {
        open = withDataUrl(open, item.image_url);
      } else {
        images.push(withDataUrl({ name: null, path: null, mediaType: "image/png", data: null }, item.image_url));
      }
      continue;
    }
    if (typeof item.text !== "string") continue;
    const trimmed = item.text.trim();
    const tag = OPEN_TAG.exec(trimmed);
    if (tag) {
      if (open) images.push(open);
      open = imageFromTag(tag[1], tag[2]);
      continue;
    }
    if (CLOSE_TAG.test(trimmed) && open) {
      images.push(open);
      open = null;
      continue;
    }
    text += extractInlineImages(item.text, images);
  }
  if (open) images.push(open);
  return { text: stripPlaceholders(text, images), images };
}

/** Rich `image` block; the host maps `source` onto a ChatAttachment. */
export function codexImageBlock(image: CodexUserImage): { kind: "image"; data: { source: Record<string, string> } } {
  const source: Record<string, string> = { media_type: image.mediaType };
  if (image.path) source.path = image.path;
  if (image.data) source.data = image.data;
  return { kind: "image", data: { source } };
}
