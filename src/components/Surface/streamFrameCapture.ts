import { frameBytes, JPEG_DECODE_BACKEND, type DecodedFrame } from "./frameFeed";
import type { Size, StreamViewport } from "./streamModel";
import type { StreamFrame } from "./surfaceStreamTransport";

function jpegSize(bytes: Uint8Array): Size | null {
  if (bytes[0] !== 0xff || bytes[1] !== 0xd8) return null;
  let offset = 2;
  while (offset + 3 < bytes.length) {
    if (bytes[offset++] !== 0xff) return null;
    while (bytes[offset] === 0xff) offset += 1;
    const marker = bytes[offset++];
    if (marker === 0xda || marker === 0xd9) return null;
    const length = (bytes[offset] << 8) | bytes[offset + 1];
    if (length < 2 || offset + length > bytes.length) return null;
    if ([0xc0, 0xc1, 0xc2, 0xc3, 0xc5, 0xc6, 0xc7, 0xc9, 0xca, 0xcb, 0xcd, 0xce, 0xcf].includes(marker)) {
      if (length < 8) return null;
      return { width: (bytes[offset + 5] << 8) | bytes[offset + 6], height: (bytes[offset + 3] << 8) | bytes[offset + 4] };
    }
    offset += length;
  }
  return null;
}

const scalar = (value: number | undefined) => value !== undefined && Number.isFinite(value) ? value : null;

export function createStreamFrameCapture(canvas: HTMLCanvasElement, stage: HTMLElement, backend: string) {
  let state: "idle" | "armed" | "capturing" | "captured" | "failed" | "disposed" = "idle";
  let generation = 0;
  let acceptedDraws = 0;
  let pending: { jpeg: Blob; metrics: object; captureId: string } | null = null;
  const urls: string[] = [];
  const panel = document.createElement("div");
  panel.setAttribute("aria-label", "Local stream capture");
  panel.style.cssText = "position:absolute;top:8px;right:8px;z-index:6;display:flex;flex-wrap:wrap;max-width:calc(100% - 32px);gap:8px;background:#111;color:white;padding:8px;font:12px sans-serif";
  const arm = document.createElement("button");
  arm.textContent = "Capture next frame";
  arm.type = "button";
  const discard = document.createElement("button");
  discard.textContent = "Discard capture";
  discard.type = "button";
  const status = document.createElement("span");
  status.setAttribute("role", "status");
  const downloads = document.createElement("span");
  panel.append(arm, discard, status, downloads);
  stage.append(panel);

  const clear = () => {
    generation += 1;
    pending = null;
    for (const url of urls) URL.revokeObjectURL(url);
    urls.length = 0;
    downloads.replaceChildren();
  };
  const reset = () => {
    clear();
    state = "idle";
    arm.disabled = false;
    status.textContent = "";
  };
  arm.onclick = () => {
    if (state !== "idle") return;
    state = "armed";
    arm.disabled = true;
    status.textContent = "Waiting for one accepted draw";
  };
  discard.onclick = reset;

  return {
    capture(next: StreamFrame, decoded: DecodedFrame, css: Size, viewport: StreamViewport | null, dpr: number) {
      if (state === "disposed") return;
      acceptedDraws += 1;
      if (state !== "armed") return;
      state = "capturing";
      const ticket = generation;
      try {
        const bytes = frameBytes(next.data);
        const jpeg = new Blob([bytes], { type: "image/jpeg" });
        const rect = canvas.getBoundingClientRect();
        const metadata = next.metadata;
        const metrics = {
          captureId: `stream-frame-${Date.now()}-${acceptedDraws}`,
          acceptedDraw: acceptedDraws,
          capturedAt: new Date().toISOString(),
          decodeBackend: backend === JPEG_DECODE_BACKEND ? backend : "custom",
          jpegBytes: bytes.byteLength,
          encoded: jpegSize(bytes),
          decoded: { width: scalar(decoded.width), height: scalar(decoded.height) },
          backing: { width: canvas.width, height: canvas.height },
          cssRect: { x: scalar(rect.x), y: scalar(rect.y), width: scalar(rect.width), height: scalar(rect.height) },
          stage: { width: scalar(css.width), height: scalar(css.height) },
          dpr: scalar(window.devicePixelRatio),
          layoutDpr: scalar(dpr),
          requestedViewport: viewport ? { w: scalar(viewport.w), h: scalar(viewport.h), dpr: scalar(viewport.dpr) } : null,
          metadata: {
            deviceWidth: scalar(metadata.deviceWidth), deviceHeight: scalar(metadata.deviceHeight),
            offsetTop: scalar(metadata.offsetTop), pageScaleFactor: scalar(metadata.pageScaleFactor),
            scrollOffsetX: scalar(metadata.scrollOffsetX), scrollOffsetY: scalar(metadata.scrollOffsetY),
            timestamp: scalar(metadata.timestamp),
          },
        };
        pending = { jpeg, metrics, captureId: metrics.captureId };
        canvas.toBlob((png) => {
          if (state === "disposed" || generation !== ticket) return;
          const captured = pending;
          pending = null;
          if (!png || !captured) {
            state = "failed";
            status.textContent = "Capture failed; discard to retry";
            return;
          }
          try {
            for (const [name, blob] of [
              ["delivered.jpg", captured.jpeg],
              ["canvas.png", png],
              ["metrics.json", new Blob([JSON.stringify(captured.metrics, null, 2)], { type: "application/json" })],
            ] as const) {
              const link = document.createElement("a");
              link.href = URL.createObjectURL(blob);
              urls.push(link.href);
              link.download = `${captured.captureId}-${name}`;
              link.textContent = `Save ${name} `;
              downloads.append(link);
            }
            state = "captured";
            status.textContent = "Captured locally";
          } catch {
            clear();
            state = "failed";
            status.textContent = "Capture failed; discard to retry";
          }
        }, "image/png");
      } catch {
        pending = null;
        state = "failed";
        status.textContent = "Capture failed; discard to retry";
      }
    },
    dispose() {
      state = "disposed";
      clear();
      arm.onclick = null;
      discard.onclick = null;
      panel.remove();
    },
  };
}
