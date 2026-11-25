import { drawAppearanceDoodles } from "./appearanceDoodleMotion";
import { appearanceLookKey, type AppearanceLook } from "./appearance";
import type { AppearanceCommit } from "./appearanceClient";
import "../components/Settings/appearanceMotion.css";

type Origin = { x: number; y: number };
type Motion = Pick<AppearanceCommit, "look" | "proposal" | "undo"> & { origin?: Origin; drawDoodles?: boolean };
type ViewTransition = { ready: Promise<void>; finished: Promise<void>; skipTransition(): void };
type ViewTransitionDocument = Document & { startViewTransition?: (update: () => void) => ViewTransition };

let stopActive: (() => void) | undefined;
let press: { button: HTMLButtonElement; origin: Origin; commit?: { proposal: string | null; peerId?: string; key: string } } | undefined;

function center({ left, top, width, height }: DOMRect): Origin {
  return { x: left + width / 2, y: top + height / 2 };
}

function reducedMotion(): boolean {
  return window.matchMedia?.("(prefers-reduced-motion: reduce), (prefers-contrast: more), (forced-colors: active)")?.matches ?? false;
}

export function indicateAppearancePress(button: HTMLButtonElement): void {
  press = { button, origin: center(button.getBoundingClientRect()) };
  if (!reducedMotion()) button.animate?.([{ transform: "scale(1)" }, { transform: "scale(.97)" }, { transform: "scale(1)" }], { duration: 120 });
}

export function clearAppearancePress(button: HTMLButtonElement): void {
  if (press?.button === button) press = undefined;
}

export function bindAppearancePress(button: HTMLButtonElement, proposal: string | null, look: AppearanceLook, peerId?: string) {
  if (press?.button === button) press.commit = { proposal, peerId, key: appearanceLookKey(look) };
}

export function takeAppearancePress(key: string, peerId?: string) {
  if (!press?.commit || press.commit.peerId !== peerId) return;
  const { commit, origin } = press;
  press = undefined;
  return key === commit.key ? { proposal: commit.proposal, origin } : undefined;
}

export function finishAppearanceMotion(): void {
  stopActive?.();
}

function visibleElement(selector: string): HTMLElement | undefined {
  return Array.from(document.querySelectorAll<HTMLElement>(selector)).find(element => {
    const { width, height, bottom, right, top, left } = element.getBoundingClientRect();
    return width > 0 && height > 0 && bottom > 0 && right > 0 && top < innerHeight && left < innerWidth
      && (element.checkVisibility?.({ opacityProperty: true, visibilityProperty: true }) ?? getComputedStyle(element).visibility === "visible");
  });
}

export function animateAppearance(motion: Motion, apply: (sync: boolean) => void): () => void {
  finishAppearanceMotion();
  const documentRoot = document.documentElement;
  const doc: ViewTransitionDocument = document;
  const fade = motion.undo || reducedMotion();
  const card = motion.proposal ? visibleElement(`[data-appearance-proposal="${CSS.escape(motion.proposal)}"]`) : undefined;
  const origin = motion.origin ?? center(visibleElement(".ct-chat-surface")?.getBoundingClientRect() ?? new DOMRect(0, 0, innerWidth, innerHeight));
  const radius = Math.hypot(Math.max(Math.abs(origin.x), Math.abs(innerWidth - origin.x)),
    Math.max(Math.abs(origin.y), Math.abs(innerHeight - origin.y)));
  let transition: ViewTransition | undefined;
  let wallpaper: HTMLElement | undefined;
  let stopDrawing: (() => void) | undefined;
  let chip: Animation | undefined;
  let fallback: Animation | undefined;
  let cancelled = false;
  let revealed = false;
  const clearTransitionStyle = () => {
    delete documentRoot.dataset.appearanceMotion;
    documentRoot.style.removeProperty("--ct-appearance-origin");
    documentRoot.style.removeProperty("--ct-appearance-radius");
  };
  const stop = () => {
    if (stopActive !== stop) return;
    stopActive = undefined;
    cancelled = true;
    transition?.skipTransition();
    chip?.cancel();
    fallback?.cancel();
    stopDrawing?.();
    if (wallpaper) delete wallpaper.dataset.drawingDoodles;
    clearTransitionStyle();
  };
  stopActive = stop;

  if (typeof doc.startViewTransition !== "function") {
    apply(false);
    fallback = document.body.animate?.([{ opacity: .8 }, { opacity: 1 }], { duration: 200, easing: "ease-out" });
    if (fallback) void fallback.finished.then(stop, stop);
    else stop();
    return stop;
  }

  documentRoot.dataset.appearanceMotion = fade ? "fade" : "wave";
  documentRoot.style.setProperty("--ct-appearance-origin", `${origin.x}px ${origin.y}px`);
  documentRoot.style.setProperty("--ct-appearance-radius", `${radius}px`);
  transition = doc.startViewTransition(() => {
    if (cancelled) return;
    apply(true);
    if (!fade && motion.drawDoodles) {
      wallpaper = visibleElement(".ct-chat-wallpaper-stack > .ct-chat-wallpaper");
      if (wallpaper) wallpaper.dataset.drawingDoodles = "";
    }
  });
  void transition.ready.then(() => {
    revealed = true;
    if (cancelled || fade || !card) return;
    chip = card.querySelector(".ct-appearance-card__state")?.animate?.(
      [{ transform: "scale(1)" }, { transform: "scale(1.08)" }, { transform: "scale(1)" }],
      { duration: 360, delay: 80, easing: "ease-out" },
    );
  }).catch(() => {});
  void transition.finished.then(() => {
    if (cancelled) return;
    transition = undefined;
    clearTransitionStyle();
    if (!revealed || !wallpaper?.isConnected) { stop(); return; }
    const bounds = wallpaper.getBoundingClientRect();
    stopDrawing = drawAppearanceDoodles(wallpaper, motion.look, [origin.x - bounds.left, origin.y - bounds.top],
      bounds.width, bounds.height, stop);
    if (!stopDrawing) stop();
  }, stop);
  return stop;
}
