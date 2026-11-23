import { drawAppearanceDoodles } from "./appearanceDoodleMotion";
import { appearanceTokens } from "./appearance";
import type { AppearanceCommit } from "./appearanceClient";
import { isLightColor, themeToCSS } from "./themeCss";
import { effectiveWallpaper, wallpaperRenderStyle } from "./chatWallpaper";
import "../components/Settings/appearanceMotion.css";

const DECEL = "cubic-bezier(.2,.7,.2,1)";
let active: { overlay: HTMLElement; animations: Animation[] } | undefined;
let pointer: { button: HTMLButtonElement; x: number; y: number } | undefined;

export function indicateAppearancePress(button: HTMLButtonElement): void {
  const area = button.getBoundingClientRect();
  pointer = { button, x: area.x + area.width / 2, y: area.y + area.height / 2 };
  if (!reducedMotion()) button.animate?.([{ transform: "scale(1)" }, { transform: "scale(.97)" }, { transform: "scale(1)" }], { duration: 120, easing: "cubic-bezier(.37,0,.63,1)" });
}

export function clearAppearancePress(button: HTMLButtonElement): void {
  if (pointer?.button === button) pointer = undefined;
}

function reducedMotion(): boolean {
  return window.matchMedia?.("(prefers-reduced-motion: reduce), (prefers-contrast: more)")?.matches ?? false;
}

export function finishAppearanceMotion(): void {
  if (!active) return;
  for (const animation of active.animations) {
    if (animation.playState !== "finished") animation.finish();
  }
  active.overlay.remove();
  active = undefined;
}

const areaOf = (selector: string) => [...document.querySelectorAll<HTMLElement>(selector)]
  .map(element => element.getBoundingClientRect())
  .find(area => area.width > 0 && area.height > 0);

function layer(overlay: HTMLElement, role: string, box?: DOMRect): HTMLElement {
  const element = document.createElement("div");
  element.className = `ct-appearance-moment__${role}`;
  if (box) {
    element.style.left = `${box.left}px`;
    element.style.top = `${box.top}px`;
    element.style.width = `${box.width}px`;
    element.style.height = `${box.height}px`;
  }
  overlay.append(element);
  return element;
}

export function animateAppearance(commit: Pick<AppearanceCommit, "look" | "proposal" | "undo">): void {
  finishAppearanceMotion();
  const pressed = pointer;
  pointer = undefined;
  if (typeof Element.prototype.animate !== "function") return;
  const reduced = commit.undo || reducedMotion();
  const chatBounds = areaOf(".ct-chat-surface") ?? areaOf("main") ?? new DOMRect(0, 0, innerWidth, innerHeight);
  const sidebar = areaOf(".ct-sidebar-panel");
  const titlebar = areaOf('[data-testid="titlebar"]');
  const composer = areaOf(".ct-composer-card");
  const card = commit.proposal ? document.querySelector<HTMLElement>(`[data-appearance-proposal="${CSS.escape(commit.proposal)}"]`) : undefined;
  const cardBounds = card?.getBoundingClientRect();
  const visibleCard = cardBounds && cardBounds.width > 0 && cardBounds.bottom > chatBounds.top && cardBounds.top < chatBounds.bottom;
  const x = pressed?.x ?? (visibleCard ? cardBounds.x + cardBounds.width / 2 : chatBounds.x + chatBounds.width / 2);
  const y = pressed?.y ?? (visibleCard ? cardBounds.y + cardBounds.height / 2 : chatBounds.y + chatBounds.height / 2);
  const origin: [number, number] = [x - chatBounds.left, y - chatBounds.top];
  const radius = Math.hypot(Math.max(Math.abs(origin[0]), Math.abs(chatBounds.width - origin[0])), Math.max(Math.abs(origin[1]), Math.abs(chatBounds.height - origin[1])));
  const overlay = document.createElement("div");
  overlay.className = "ct-appearance-moment";
  overlay.setAttribute("aria-hidden", "true");
  const theme = commit.look.theme;
  const paper = effectiveWallpaper(theme, commit.look.wallpaper ?? undefined);
  const drawing = !reduced && paper.background.pattern === "doodles";
  const light = isLightColor(theme.background);
  const paperStyle = wallpaperRenderStyle(theme, drawing ? {
    ...paper, background: { ...paper.background, pattern: "none" },
  } : commit.look.wallpaper ?? undefined, light);
  const tokens = { ...themeToCSS(theme), ...appearanceTokens(commit.look.tuning), ...paperStyle };
  for (const [key, value] of Object.entries(tokens)) overlay.style.setProperty(key, value);
  overlay.dataset.ctTheme = isLightColor(theme.background) ? "light" : "dark";
  const frame = layer(overlay, "frame");
  const chat = layer(overlay, "chat", chatBounds);
  const backdrop = document.createElement("span");
  backdrop.className = "ct-chat-wallpaper";
  chat.append(backdrop);
  const sidebarLayer = layer(overlay, "sidebar", sidebar);
  const chrome = layer(overlay, "chrome");
  if (titlebar) layer(chrome, "titlebar", titlebar);
  if (composer) layer(chrome, "composer", composer);
  document.body.append(overlay);
  const fade = [{ opacity: 0 }, { opacity: 1 }];
  const animations = reduced
    ? [overlay.animate(fade, { duration: 200, fill: "both" })]
    : [
      chat.animate([{ clipPath: `circle(0px at ${origin[0]}px ${origin[1]}px)` }, { clipPath: `circle(${radius}px at ${origin[0]}px ${origin[1]}px)` }], { duration: 560, fill: "both", easing: DECEL }),
      frame.animate(fade, { delay: 180, duration: 360, fill: "both", easing: DECEL }),
      ...(sidebar ? [sidebarLayer.animate(fade, { delay: 180, duration: 360, fill: "both", easing: DECEL })] : []),
      chrome.animate(fade, { delay: 260, duration: 360, fill: "both", easing: DECEL }),
      ...(drawing ? drawAppearanceDoodles(chat, commit.look, origin, chatBounds.width, chatBounds.height) : []),
      ...(visibleCard ? [card!.querySelector(".ct-appearance-card__state")?.animate(
        [{ transform: "scale(1)" }, { transform: "scale(1.08)" }, { transform: "scale(1)" }],
        { duration: 360, delay: 80, easing: "ease-out" },
      )].filter((value): value is Animation => value !== undefined) : []),
    ];
  active = { overlay, animations };
  void Promise.all(animations.map(animation => animation.finished)).finally(() => {
    overlay.remove();
    if (active?.overlay === overlay) active = undefined;
  }).catch(() => {});
}
