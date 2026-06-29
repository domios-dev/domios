// Per-node bitmap cache. Each node is rasterized ONCE to an offscreen canvas at its
// on-screen logical size × dpr (crisp text) via its painter, keyed by a stable content
// signature. Pan re-blits cached bitmaps (cheap); a node re-rasterizes only when its
// signature changes or — at gesture-end resharpen — its on-screen size drifts.

import type { HitRegion } from "./drawCard";

export type PaintFn = (ctx: CanvasRenderingContext2D) => HitRegion[];

export interface CachedBitmap {
  canvas: HTMLCanvasElement;
  w: number;
  h: number;
  signature: string;
  dpr: number;
  regions: HitRegion[];
}

const RESIZE_TOLERANCE = 0.14;

export class NodeBitmapCache {
  private map = new Map<string, CachedBitmap>();

  get(id: string): CachedBitmap | undefined {
    return this.map.get(id);
  }

  private render(id: string, signature: string, w: number, h: number, dpr: number, paint: PaintFn): CachedBitmap {
    const canvas = this.map.get(id)?.canvas ?? document.createElement("canvas");
    const pw = Math.max(1, Math.ceil(w * dpr));
    const ph = Math.max(1, Math.ceil(h * dpr));
    if (canvas.width !== pw) canvas.width = pw;
    if (canvas.height !== ph) canvas.height = ph;
    const ctx = canvas.getContext("2d");
    let regions: HitRegion[] = [];
    if (ctx) {
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, w, h);
      regions = paint(ctx);
    }
    const entry: CachedBitmap = { canvas, w, h, signature, dpr, regions };
    this.map.set(id, entry);
    return entry;
  }

  // Blit-loop path: render only when missing / signature / dpr changed — never on size
  // alone (a zoom gesture scale-blits the existing bitmap, staying smooth).
  ensure(id: string, signature: string, w: number, h: number, dpr: number, paint: PaintFn): CachedBitmap {
    const cur = this.map.get(id);
    if (cur && cur.signature === signature && cur.dpr === dpr) return cur;
    return this.render(id, signature, w, h, dpr, paint);
  }

  // Gesture-settle path: also re-rasterize when the on-screen size drifted past the
  // tolerance, restoring 1:1 crispness after a continuous zoom.
  resharpen(id: string, signature: string, w: number, h: number, dpr: number, paint: PaintFn): CachedBitmap {
    const cur = this.map.get(id);
    const sizeOk = cur && Math.abs(cur.w - w) <= cur.w * RESIZE_TOLERANCE && Math.abs(cur.h - h) <= cur.h * RESIZE_TOLERANCE;
    if (cur && cur.signature === signature && cur.dpr === dpr && sizeOk) return cur;
    return this.render(id, signature, w, h, dpr, paint);
  }

  prune(keep: Set<string>): void {
    if (this.map.size <= keep.size + 64) return;
    for (const id of this.map.keys()) if (!keep.has(id)) this.map.delete(id);
  }

  clear(): void {
    this.map.clear();
  }
}
