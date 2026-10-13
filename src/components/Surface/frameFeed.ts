import { decodeBase64 } from "./streamModel";
import type { StreamFrame } from "./surfaceStreamTransport";

export type FrameListener = (frame: StreamFrame) => void;

export interface FrameFeed {
  subscribe(listener: FrameListener): () => void;
}

export interface FramePublisher extends FrameFeed {
  publish(frame: StreamFrame): void;
}

export function createFrameFeed(): FramePublisher {
  const listeners = new Set<FrameListener>();
  let latest: StreamFrame | null = null;
  return {
    subscribe(listener) {
      listeners.add(listener);
      if (latest) listener(latest);
      return () => { listeners.delete(listener); };
    },
    publish(frame) {
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

export function frameBytes(data: StreamFrame["data"]): Uint8Array<ArrayBuffer> {
  if (typeof data !== "string") return new Uint8Array(data);
  const decoded = decodeBase64(data);
  return new Uint8Array(decoded.buffer as ArrayBuffer, decoded.byteOffset, decoded.byteLength);
}

export const decodeJpegFrame: FrameDecoder = async (frame) => {
  const bitmap = await createImageBitmap(new Blob([frameBytes(frame.data)], { type: "image/jpeg" }));
  return { width: bitmap.width, height: bitmap.height, image: bitmap, close: () => bitmap.close() };
};
