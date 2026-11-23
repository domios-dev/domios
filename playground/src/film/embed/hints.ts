import { useEffect, useRef } from "react";

export const HINT_ATTRIBUTE = "data-film-hint";

export interface Box { left: number; top: number; width: number; height: number }
export interface HintTarget { hint: string; rect: Box }

/** Above this share of the stage a hinted element is the shot itself: it names itself without an outline. */
export const WIDE_SHARE = 0.6;

const inside = (r: Box, x: number, y: number) => r.width > 0 && r.height > 0 && x >= r.left && x <= r.left + r.width && y >= r.top && y <= r.top + r.height;

/** The innermost hinted element under the point: the smallest one that contains it. */
export function pickHint(targets: HintTarget[], x: number, y: number): HintTarget | null {
  let best: HintTarget | null = null;
  for (const target of targets) {
    if (!inside(target.rect, x, y)) continue;
    if (!best || target.rect.width * target.rect.height < best.rect.width * best.rect.height) best = target;
  }
  return best;
}

/** Splits "Title: description" so the chip can set the title apart. */
export function splitHint(hint: string): [string, string] {
  const at = hint.indexOf(": ");
  return at < 0 ? [hint, ""] : [hint.slice(0, at), hint.slice(at + 2)];
}

/**
 * Follows a mouse over the playing stage and names what is under it. The scene layer stays inert, so this hit-tests the
 * hinted elements' boxes itself, once per frame while the pointer is over the stage, and writes the chip straight to the DOM.
 */
export function useHints(stage: HTMLElement | null, enabled: boolean) {
  const chip = useRef<HTMLDivElement>(null);
  const ring = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const chipEl = chip.current;
    const ringEl = ring.current;
    if (!stage || !chipEl || !ringEl || !enabled) return;
    const [title, detail] = [chipEl.children[0] as HTMLElement, chipEl.children[1] as HTMLElement];
    let pointer: { x: number; y: number } | null = null;
    let shown = "";
    let raf = 0;

    const hide = () => {
      delete chipEl.dataset.on;
      delete ringEl.dataset.on;
      shown = "";
    };
    const frame = () => {
      raf = 0;
      if (!pointer) return hide();
      const origin = stage.getBoundingClientRect();
      const targets: HintTarget[] = [];
      for (const el of stage.querySelectorAll<HTMLElement>(`[${HINT_ATTRIBUTE}]`)) {
        const r = el.getBoundingClientRect();
        targets.push({ hint: el.getAttribute(HINT_ATTRIBUTE) ?? "", rect: { left: r.left - origin.left, top: r.top - origin.top, width: r.width, height: r.height } });
      }
      const x = pointer.x - origin.left;
      const y = pointer.y - origin.top;
      const hit = pickHint(targets, x, y);
      if (!hit) hide();
      else {
        if (hit.hint !== shown) {
          shown = hit.hint;
          [title.textContent, detail.textContent] = splitHint(hit.hint);
        }
        const flip = x > origin.width - 260;
        chipEl.style.transform = `translate(${x + (flip ? -14 : 14)}px, ${y + 18}px) translateX(${flip ? "-100%" : "0"})`;
        chipEl.dataset.on = "";
        const r = hit.rect;
        if (r.width * r.height > WIDE_SHARE * origin.width * origin.height) delete ringEl.dataset.on;
        else {
          Object.assign(ringEl.style, { left: `${r.left}px`, top: `${r.top}px`, width: `${r.width}px`, height: `${r.height}px` });
          ringEl.dataset.on = "";
        }
      }
      raf = requestAnimationFrame(frame);
    };
    const move = (e: PointerEvent) => {
      if (e.pointerType === "touch") return;
      pointer = { x: e.clientX, y: e.clientY };
      if (!raf) raf = requestAnimationFrame(frame);
    };
    const leave = () => {
      pointer = null;
      cancelAnimationFrame(raf);
      raf = 0;
      hide();
    };
    stage.addEventListener("pointermove", move);
    stage.addEventListener("pointerleave", leave);
    return () => {
      stage.removeEventListener("pointermove", move);
      stage.removeEventListener("pointerleave", leave);
      leave();
    };
  }, [stage, enabled]);

  return { chip, ring };
}
