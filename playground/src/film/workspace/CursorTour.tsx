import { useEffect, useRef } from "react";
import { dwellBefore, hopDuration, LOOP_HOLD_MS, PRESS_MS, RING_MS, STOP_FADE_MS, type TourStep } from "./tour";

const FIND_MS = 3000;
const EASE = "cubic-bezier(0.45, 0, 0.25, 1)";
const point = (x: number, y: number) => ({ left: `${x}px`, top: `${y}px` });

class Stopped extends Error {}

/** Animation pacing for the tour's own motion; an abort ends the wait at once. */
function wait(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    if (signal.aborted) return reject(new Stopped());
    const abort = () => { clearTimeout(timer); reject(new Stopped()); };
    const timer = setTimeout(() => { signal.removeEventListener("abort", abort); resolve(); }, ms);
    signal.addEventListener("abort", abort, { once: true });
  });
}

/** The step's control once it is on screen inside the workspace; null when it never shows up. */
function find(host: HTMLElement, step: TourStep, signal: AbortSignal): Promise<HTMLElement | null> {
  const deadline = performance.now() + FIND_MS;
  return new Promise((resolve, reject) => {
    const look = () => {
      if (signal.aborted) return reject(new Stopped());
      // Menus portal out of the workspace tree into the stage, so the search covers the whole film root.
      const elements = (host.getRootNode() as ParentNode).querySelectorAll<HTMLElement>(step.target);
      for (const el of elements) {
        const label = el.textContent?.trim() ?? "";
        if (step.label && (typeof step.label === "string" ? step.label !== label : !step.label.includes(label))) continue;
        const box = el.getBoundingClientRect();
        if (!box.width || !box.height || el.matches(":disabled, [aria-disabled=true]")) continue;
        const scroller = el.closest<HTMLElement>(".ct-sidebar-content");
        if (scroller) {
          const bounds = scroller.getBoundingClientRect();
          if (box.bottom > bounds.bottom) scroller.scrollTop += box.bottom - bounds.bottom + 8;
          if (box.top < bounds.top) scroller.scrollTop -= bounds.top - box.top + 8;
        }
        return resolve(el);
      }
      if (performance.now() > deadline) return resolve(null);
      requestAnimationFrame(look);
    };
    look();
  });
}

function pointer(el: HTMLElement, type: "pointerdown" | "pointerup", x: number, y: number) {
  el.dispatchEvent(new PointerEvent(type, { bubbles: true, composed: true, clientX: x, clientY: y, pointerType: "mouse", button: 0, buttons: type === "pointerdown" ? 1 : 0 }));
}

export interface CursorTourProps {
  host: HTMLElement | null;
  active: boolean;
  steps: TourStep[];
  guided: boolean;
  onFinish: (completed: boolean) => void;
}

/**
 * A calm cursor that walks the app's own controls and really clicks them, in a loop. The first real input inside the
 * frame stops it where it is. An explicit guide can run once after that.
 */
export function CursorTour({ host, active, steps, guided, onFinish }: CursorTourProps) {
  const cursor = useRef<HTMLDivElement>(null);
  const ring = useRef<HTMLDivElement>(null);
  const glyph = useRef<SVGSVGElement>(null);
  const index = useRef(0);
  const position = useRef({ x: 0, y: 0, shown: false });
  const stopped = useRef(false);

  useEffect(() => {
    const el = cursor.current;
    if (!host || !el || !active || (!guided && stopped.current)) return;
    const controller = new AbortController();
    if (guided) index.current = 0;
    const inputRoot = host.closest(".film-workspace") ?? host;

    const run = async (signal: AbortSignal) => {
      if (!guided && !position.current.shown) await wait(7500, signal);
      el.style.transition = `opacity ${STOP_FADE_MS}ms linear`;
      el.style.opacity = "1";
      for (;;) {
        const step = steps[index.current];
        const target = await find(host, step, signal);
        if (!target && guided) { onFinish(false); return; }
        if (target) {
          const frame = host.getBoundingClientRect();
          const box = target.getBoundingClientRect();
          const x = box.left - frame.left + box.width / 2;
          const y = box.top - frame.top + box.height / 2;
          const from = position.current.shown ? position.current : { x: x + 42, y: y + 30 };
          const dx = x - from.x;
          const dy = y - from.y;
          const direction = index.current % 2 === 0 ? 1 : -1;
          const bend = Math.min(0.16, 48 / Math.max(1, Math.hypot(dx, dy))) * direction;
          const hop = Math.round(hopDuration(dx, dy) * (guided ? 0.55 : 0.85));
          const targetPoint = point(x, y);
          const motion = el.animate([
            point(from.x, from.y),
            point(from.x + dx * 0.57 - dy * bend, from.y + dy * 0.57 + dx * bend),
            targetPoint,
          ], { duration: hop, easing: EASE });
          Object.assign(el.style, targetPoint);
          position.current = { x, y, shown: true };
          try { await wait(hop, signal); } finally { motion.cancel(); }
          await wait(guided ? 175 : dwellBefore(index.current), signal);
          if (!target.isConnected) {
            if (guided) { onFinish(false); return; }
            index.current = (index.current + 1) % steps.length;
            continue;
          }
          const latest = target.getBoundingClientRect();
          const latestFrame = host.getBoundingClientRect();
          const cx = latest.left + latest.width / 2;
          const cy = latest.top + latest.height / 2;
          const tapX = cx - latestFrame.left;
          const tapY = cy - latestFrame.top;
          const tapPoint = point(tapX, tapY);
          if (Math.hypot(tapX - x, tapY - y) > 1) {
            const aim = el.animate([targetPoint, tapPoint], { duration: 115, easing: "ease-out" });
            Object.assign(el.style, tapPoint);
            try { await wait(115, signal); } finally { aim.cancel(); }
          } else {
            Object.assign(el.style, tapPoint);
          }
          position.current = { x: tapX, y: tapY, shown: true };
          glyph.current?.animate([{ opacity: 1 }, { opacity: 0.72 }, { opacity: 1 }], {
            duration: PRESS_MS * 2, easing: "ease-out",
          });
          pointer(target, "pointerdown", cx, cy);
          await wait(PRESS_MS, signal);
          pointer(target, "pointerup", cx, cy);
          target.click();
          const r = ring.current;
          if (r) {
            Object.assign(r.style, tapPoint);
            r.animate(
              [{ opacity: 0.55, width: "12px", height: "12px" }, { opacity: 0, width: "34px", height: "34px" }],
              { duration: RING_MS, easing: "ease-out" },
            );
          }
          await wait(step.look ?? 0, signal);
        }
        index.current = (index.current + 1) % steps.length;
        if (index.current === 0) {
          if (guided) { await wait(300, signal); onFinish(true); return; }
          await wait(LOOP_HOLD_MS, signal);
        }
      }
    };

    // Only the visitor's own input stops the tour; the tour's synthetic events are not trusted, and page scroll is not input.
    const onInput = (event: Event) => {
      if (!event.isTrusted) return;
      controller.abort();
      stopped.current = true;
      if (guided) onFinish(true);
      el.style.transition = `opacity ${STOP_FADE_MS}ms linear`;
      el.style.opacity = "0";
    };
    const events = ["pointermove", "pointerdown", "wheel", "keydown", "touchstart"] as const;
    const scope = inputRoot.getRootNode();
    for (const type of events) scope.addEventListener(type, onInput, { capture: true, passive: true });
    void run(controller.signal).catch((error: unknown) => {
      if (!(error instanceof Stopped)) throw error;
    });
    return () => {
      for (const type of events) scope.removeEventListener(type, onInput, { capture: true });
      controller.abort();
      el.style.opacity = "0";
    };
  }, [host, active, steps, guided, onFinish]);

  return (
    <div aria-hidden="true" style={{ position: "absolute", inset: 0, pointerEvents: "none", zIndex: 2147483000, overflow: "hidden" }}>
      <div ref={ring} style={{ position: "absolute", left: 0, top: 0, width: 28, height: 28, transform: "translate(-50%, -50%)", borderRadius: 999, border: "2px solid var(--ct-accent)", opacity: 0 }} />
      <div ref={cursor} style={{ position: "absolute", left: 0, top: 0, opacity: 0 }}>
        <svg ref={glyph} width="18" height="24" viewBox="0 0 18 24" style={{ display: "block", transform: "translate(-2px, -1.5px)", transformOrigin: "0 0" }}>
          <path d="M2 1.5v17.2l4.4-4.1 2.9 6.6 2.7-1.2-2.9-6.5h6.1z" fill="#fff" stroke="#111" strokeWidth="1.3" strokeLinejoin="round" />
        </svg>
      </div>
    </div>
  );
}
