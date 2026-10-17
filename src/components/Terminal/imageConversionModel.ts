export function imageConversionPlan(name: string, mime: string, capability: { bitmap: boolean; image: boolean }) {
  const heic = /\.(heic|heif)$/i.test(name) || /^(image\/|public\.)(heic|heif)(-sequence)?$/i.test(mime);
  return { convert: heic, decoders: heic ? [...(capability.bitmap ? ["bitmap" as const] : []), ...(capability.image ? ["image" as const] : [])] : [], name: heic ? `${name.replace(/\.[^.]*$/, "") || "image"}.jpg` : name };
}

export function jpegConversionAccepted(result: { type: string; size: number } | null): boolean {
  return result !== null && result.type === "image/jpeg" && result.size > 0;
}
