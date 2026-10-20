import { imageConversionPlan, jpegConversionAccepted } from "./imageConversionModel";

type DecodedImage = { source: CanvasImageSource; width: number; height: number; release: () => void };

async function decodeImage(blob: Blob, decoder: "bitmap" | "image"): Promise<DecodedImage> {
  if (decoder === "bitmap") {
    const bitmap = await createImageBitmap(blob);
    return { source: bitmap, width: bitmap.width, height: bitmap.height, release: () => bitmap.close() };
  }
  const url = URL.createObjectURL(blob);
  try {
    const image = new Image();
    await new Promise<void>((resolve, reject) => {
      image.onload = () => resolve();
      image.onerror = () => reject(new Error("Image decoding failed"));
      image.src = url;
    });
    return { source: image, width: image.naturalWidth, height: image.naturalHeight, release: () => URL.revokeObjectURL(url) };
  } catch (error) {
    URL.revokeObjectURL(url);
    throw error;
  }
}

export async function prepareBrowserUpload(blob: Blob, name: string): Promise<{ blob: Blob; name: string }> {
  const plan = imageConversionPlan(name, blob.type, { bitmap: typeof createImageBitmap === "function", image: typeof Image !== "undefined" });
  if (!plan.convert) return { blob, name };
  for (const decoder of plan.decoders) {
    let decoded: DecodedImage | undefined;
    try {
      decoded = await decodeImage(blob, decoder);
      const canvas = document.createElement("canvas");
      canvas.width = decoded.width;
      canvas.height = decoded.height;
      const context = canvas.getContext("2d");
      if (!context) continue;
      context.drawImage(decoded.source, 0, 0);
      const jpeg = await new Promise<Blob | null>(resolve => canvas.toBlob(resolve, "image/jpeg", 0.92));
      if (jpegConversionAccepted(jpeg)) return { blob: new File([jpeg!], plan.name, { type: "image/jpeg" }), name: plan.name };
    } catch {
      continue;
    } finally {
      decoded?.release();
    }
  }
  throw new Error("This browser cannot convert this HEIC image. Attach a JPEG or PNG instead.");
}
