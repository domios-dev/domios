import { decodeBase64 } from "./streamModel";
import type { StreamFrame } from "./surfaceStreamTransport";

export type FrameListener = (frame: StreamFrame) => void;

export interface FrameFeed {
  subscribe(listener: FrameListener): () => void;
  accepts?(frame: StreamFrame): boolean;
}

export interface FramePublisher extends FrameFeed {
  publish(frame: StreamFrame): void;
}

export function createFrameFeed(accepts: (frame: StreamFrame) => boolean = () => true): FramePublisher {
  const listeners = new Set<FrameListener>();
  let latest: StreamFrame | null = null;
  return {
    accepts,
    subscribe(listener) {
      listeners.add(listener);
      if (latest && accepts(latest)) listener(latest);
      return () => { listeners.delete(listener); };
    },
    publish(frame) {
      if (!accepts(frame)) return;
      latest = frame;
      for (const listener of listeners) listener(frame);
    },
  };
}

export interface DecodedFrame {
  width: number;
  height: number;
  image: CanvasImageSource;
  close(): void;
}

export type FrameDecoder = (frame: StreamFrame) => Promise<DecodedFrame>;

export const JPEG_DECODE_BACKEND = "HTMLImageElement/jpeg";

export function frameBytes(data: StreamFrame["data"]): Uint8Array<ArrayBuffer> {
  if (typeof data !== "string") return new Uint8Array(data);
  const decoded = decodeBase64(data);
  return new Uint8Array(decoded.buffer as ArrayBuffer, decoded.byteOffset, decoded.byteLength);
}

export const decodeJpegFrame: FrameDecoder = async (frame) => {
  const image = new Image();
  if (typeof image.decode !== "function") throw new Error("JPEG image decoding is unavailable");
  const url = URL.createObjectURL(new Blob([frameBytes(frame.data)], { type: "image/jpeg" }));
  let closed = false;
  const close = () => {
    if (closed) return;
    closed = true;
    URL.revokeObjectURL(url);
  };
  try {
    image.src = url;
    await image.decode();
    return { width: image.naturalWidth, height: image.naturalHeight, image, close };
  } catch (error) {
    close();
    throw error;
  }
};
