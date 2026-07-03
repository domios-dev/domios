export interface ExtractedToolImage {
  mimeType: string;
  dataUrl: string;
  base64: string;
}

const DATA_URL_PATTERN =
  /^data:(image\/[a-zA-Z0-9.+-]+);base64,([A-Za-z0-9+/=\s]+)$/;

const EMBEDDED_DATA_URL_PATTERN =
  /data:image\/[a-zA-Z0-9.+-]+;base64,[A-Za-z0-9+/=\s]+/g;

function parseDataUrl(raw: string): ExtractedToolImage | null {
  const trimmed = raw.trim();
  const match = trimmed.match(DATA_URL_PATTERN);
  if (!match) return null;
  const base64 = match[2].replace(/\s/g, "");
  return {
    mimeType: match[1],
    base64,
    dataUrl: `data:${match[1]};base64,${base64}`,
  };
}

function tryParseImageUrlJson(text: string): ExtractedToolImage[] | null {
  const trimmed = text.trim();
  if (!trimmed.startsWith("[") && !trimmed.startsWith("{")) return null;
  try {
    const parsed = JSON.parse(trimmed) as unknown;
    const items = Array.isArray(parsed) ? parsed : [parsed];
    const images: ExtractedToolImage[] = [];
    for (const item of items) {
      if (!item || typeof item !== "object") continue;
      const record = item as Record<string, unknown>;
      if (record.type !== "image_url") continue;
      const imageUrl = record.image_url;
      if (!imageUrl || typeof imageUrl !== "object") continue;
      const url = (imageUrl as Record<string, unknown>).url;
      if (typeof url !== "string") continue;
      const image = parseDataUrl(url);
      if (image) images.push(image);
    }
    return images.length > 0 ? images : null;
  } catch {
    return null;
  }
}

export interface ParsedToolOutputImages {
  images: ExtractedToolImage[];
  remainder: string | null;
}

export function parseToolOutputImages(text: string): ParsedToolOutputImages {
  const jsonImages = tryParseImageUrlJson(text);
  if (jsonImages) {
    return { images: jsonImages, remainder: null };
  }

  const soleImage = parseDataUrl(text);
  if (soleImage) {
    return { images: [soleImage], remainder: null };
  }

  const images: ExtractedToolImage[] = [];
  let remainder = text;
  for (const match of text.matchAll(EMBEDDED_DATA_URL_PATTERN)) {
    const image = parseDataUrl(match[0]);
    if (!image) continue;
    images.push(image);
    remainder = remainder.replace(match[0], "");
  }

  if (images.length === 0) {
    return { images: [], remainder: text };
  }

  const cleaned = remainder.replace(/\s+/g, " ").trim();
  return { images, remainder: cleaned.length > 0 ? cleaned : null };
}

export function isImageOnlyToolOutput(text: string): boolean {
  const { images, remainder } = parseToolOutputImages(text);
  return images.length > 0 && !remainder;
}

export function toolResultPreviewLine(text: string): string {
  const { images, remainder } = parseToolOutputImages(text);
  if (images.length > 0) {
    const ext = images[0].mimeType.replace("image/", "").toUpperCase();
    if (!remainder) {
      return images.length === 1 ? `Image (${ext})` : `${images.length} images`;
    }
    const tail = remainder.split("\n").map((line) => line.trim()).filter(Boolean).at(-1);
    if (tail) return tail;
    return images.length === 1 ? `Image (${ext})` : `${images.length} images`;
  }
  return text
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean)
    .at(-1) ?? text;
}
