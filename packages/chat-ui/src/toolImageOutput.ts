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

function tryParseImageUrlJson(text: string): ParsedToolOutputImages | null {
  const trimmed = text.trim();
  if (!trimmed.startsWith("[") && !trimmed.startsWith("{")) return null;
  try {
    const parsed = JSON.parse(trimmed) as unknown;
    const items: unknown[] = Array.isArray(parsed) ? parsed : [parsed];
    const images: ExtractedToolImage[] = [];
    const remainder: string[] = [];
    for (const item of items) {
      if (!item || typeof item !== "object") { remainder.push(JSON.stringify(item)); continue; }
      const record = item as Record<string, unknown>;
      let url: string | undefined;
      if (record.type === "image_url" || record.type === "input_image") {
        const value = record.image_url;
        url = typeof value === "string" ? value : value && typeof value === "object"
          && typeof (value as Record<string, unknown>).url === "string" ? (value as { url: string }).url : undefined;
      } else if (record.type === "image" && record.source && typeof record.source === "object") {
        const source = record.source as Record<string, unknown>;
        if (typeof source.media_type === "string" && typeof source.data === "string") url = `data:${source.media_type};base64,${source.data}`;
      }
      const image = url ? parseDataUrl(url) : null;
      if (image) images.push(image);
      else remainder.push(typeof record.text === "string" ? record.text : JSON.stringify(item));
    }
    return images.length > 0 ? { images, remainder: remainder.join("\n").trim() || null } : null;
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
    return jsonImages;
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
    const lines = remainder.split("\n").map((line) => line.trim()).filter(Boolean);
    const tail = lines[lines.length - 1];
    if (tail) return tail;
    return images.length === 1 ? `Image (${ext})` : `${images.length} images`;
  }
  const lines = text
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
  return lines[lines.length - 1] ?? text;
}
