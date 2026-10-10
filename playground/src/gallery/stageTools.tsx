import { useEffect, useRef, type ReactNode } from "react";
import { SHELL_NARROW_MAX_PX, SHELL_NARROW_QUERY } from "../../../src/lib/shellBreakpoint";

const PSEUDO = [
  [":hover", "gallery-pseudo-hover"],
  [":focus-within", "gallery-pseudo-focus-within"],
  [":focus-visible", "gallery-pseudo-focus-visible"],
  [":active", "gallery-pseudo-active"],
] as const;

export type PseudoState = "hover" | "focus-within" | "focus-visible" | "active";

let mirrored = false;

function mirrorRules(list: CSSRuleList, parent: CSSStyleSheet | CSSGroupingRule): void {
  for (const rule of Array.from(list)) {
    if (rule instanceof CSSStyleRule) {
      const text = rule.cssText;
      if (!PSEUDO.some(([pseudo]) => text.includes(pseudo))) continue;
      const forced = PSEUDO.reduce((css, [pseudo, cls]) => css.split(pseudo).join(`.${cls}`), text);
      try {
        parent.insertRule(forced, parent.cssRules.length);
      } catch {
        // Selectors the engine rejects after rewriting stay unmirrored.
      }
    } else if ("cssRules" in rule && (rule instanceof CSSMediaRule || rule instanceof CSSSupportsRule || rule instanceof CSSLayerBlockRule)) {
      mirrorRules(rule.cssRules, rule);
    }
  }
}

/** Mirrors every shipped :hover / :focus-within / :focus-visible rule onto a class, so a static stage can show it. */
function mirrorPseudoRules(): void {
  if (mirrored) return;
  mirrored = true;
  for (const sheet of Array.from(document.styleSheets)) {
    try {
      mirrorRules(sheet.cssRules, sheet);
    } catch {
      // Cross-origin sheets are not readable.
    }
  }
}

/** Forces a mirrored pseudo-state onto one element, e.g. a portalled trigger outside any ForcePseudo subtree. */
export function forcePseudo(target: Element, state: PseudoState): void {
  mirrorPseudoRules();
  const cls = PSEUDO.find(([pseudo]) => pseudo === `:${state}`)?.[1];
  if (cls && !target.classList.contains(cls)) target.classList.add(cls);
}

/** Applies a forced pseudo-state class to the first element matching `selector` inside the children. */
export function ForcePseudo({ selector, state, children }: { selector: string; state: PseudoState; children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const apply = () => {
      const target = ref.current?.querySelector(selector);
      if (target) forcePseudo(target, state);
    };
    apply();
    const observer = new MutationObserver(apply);
    if (ref.current) observer.observe(ref.current, { childList: true, subtree: true });
    return () => observer.disconnect();
  }, [selector, state]);
  return <div ref={ref} className="gallery-fill">{children}</div>;
}

const DROPDOWN_OPEN_EVENT = "codeterm:dropdown-open";
let pinning = false;

// Product dropdowns close each other when one opens; pinned stage menus stay open side by side.
window.addEventListener(DROPDOWN_OPEN_EVENT, (event) => {
  if (pinning) event.stopImmediatePropagation();
}, true);

function stageOf(element: HTMLElement | null): HTMLElement | null {
  return element?.closest<HTMLElement>(".gallery-stage-instance") ?? null;
}

function openMenu(trigger: HTMLElement, via: "pointerdown" | "click"): void {
  pinning = true;
  try {
    if (via === "click") trigger.click();
    else trigger.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, cancelable: true }));
  } finally {
    pinning = false;
  }
}

function nextFrame(): Promise<void> {
  return new Promise((resolve) => requestAnimationFrame(() => resolve()));
}

export async function waitFor<T>(read: () => T | null | undefined, attempts = 60): Promise<T | null> {
  for (let attempt = 0; attempt < attempts; attempt += 1) {
    const value = read();
    if (value) return value;
    await nextFrame();
  }
  return null;
}

/**
 * Opens the product dropdown whose trigger matches `trigger` and keeps it open; an
 * optional `submenu` label pages into that submenu. The menu renders in the stage.
 */
export function PinnedMenu({ trigger, submenu, via = "pointerdown", menuSelector = ".ct-portal-menu", children }: {
  trigger: string;
  submenu?: string;
  via?: "pointerdown" | "click";
  menuSelector?: string;
  children: ReactNode;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    let alive = true;
    let reopenTimer = 0;
    const stage = stageOf(ref.current);
    const menu = () => stage?.querySelector<HTMLElement>(menuSelector) ?? null;

    const pin = async () => {
      const button = await waitFor(() => ref.current?.querySelector<HTMLElement>(trigger));
      if (!alive || !button || menu()) return;
      openMenu(button, via);
      const opened = await waitFor(menu);
      if (!alive || !opened || !submenu) return;
      const row = await waitFor(() => Array.from(opened.querySelectorAll<HTMLElement>("[data-menu-key], [role=menuitem], button"))
        .find((element) => element.textContent?.trim().startsWith(submenu)));
      row?.click();
    };

    void pin();
    const observer = new MutationObserver(() => {
      if (menu() || reopenTimer) return;
      reopenTimer = window.setTimeout(() => {
        reopenTimer = 0;
        void pin();
      }, 400);
    });
    if (stage) observer.observe(stage, { childList: true, subtree: true });
    return () => {
      alive = false;
      window.clearTimeout(reopenTimer);
      observer.disconnect();
    };
  }, [trigger, submenu, via, menuSelector]);
  return <div ref={ref} className="gallery-fill">{children}</div>;
}

const PHONE_SCOPE = "gallery-phone-viewport";
const phoneSpecimens = new Set<string>();
let phoneMediaMirrored = false;

function phoneRoute(): boolean {
  const route = window.location.hash.replace(/^#gallery\//, "");
  return phoneSpecimens.has(route);
}

function forcedMediaList(query: string): MediaQueryList {
  return {
    matches: true,
    media: query,
    onchange: null,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
    addListener: () => undefined,
    removeListener: () => undefined,
    dispatchEvent: () => false,
  };
}

function splitSelectors(text: string): string[] {
  const parts: string[] = [];
  let depth = 0;
  let start = 0;
  for (let index = 0; index < text.length; index += 1) {
    const char = text[index];
    if (char === "(" || char === "[") depth += 1;
    else if (char === ")" || char === "]") depth -= 1;
    else if (char === "," && depth === 0) {
      parts.push(text.slice(start, index).trim());
      start = index + 1;
    }
  }
  parts.push(text.slice(start).trim());
  return parts;
}

function scopeSelector(selector: string): string {
  const root = /^(:root|html|body)\b/.exec(selector);
  return root ? `.${PHONE_SCOPE}${selector.slice(root[0].length)}` : `.${PHONE_SCOPE} ${selector}`;
}

function appliesOnPhone(condition: string): boolean {
  if (/^\s*not\b/.test(condition)) return false;
  return condition.split(",").some((query) => {
    const max = /(?:max-width:\s*|width\s*<=?\s*)(\d+)px/.exec(query);
    return Boolean(max && Number(max[1]) >= SHELL_NARROW_MAX_PX && !/min-width|width\s*>/.test(query));
  });
}

function mirrorPhoneRules(list: CSSRuleList, parent: CSSStyleSheet | CSSGroupingRule): void {
  for (const rule of Array.from(list)) {
    if (rule instanceof CSSMediaRule && appliesOnPhone(rule.conditionText)) {
      for (const inner of Array.from(rule.cssRules)) {
        if (!(inner instanceof CSSStyleRule)) continue;
        const selector = splitSelectors(inner.selectorText).map(scopeSelector).join(", ");
        try {
          parent.insertRule(`${selector} { ${inner.style.cssText} }`, parent.cssRules.length);
        } catch {
          // Selectors the engine rejects after scoping stay unmirrored.
        }
      }
    } else if (rule instanceof CSSLayerBlockRule || rule instanceof CSSSupportsRule) {
      mirrorPhoneRules(rule.cssRules, rule);
    }
  }
}

/**
 * Makes `specimenId` render as a phone (max-width 640px) inside the desktop gallery: the product's
 * SHELL_NARROW matchMedia answers true on that route only, and phone-width @media rules are mirrored
 * under the wrapper so the stage gets the phone stylesheet without an iframe.
 */
export function forcePhoneViewport(specimenId: string): void {
  phoneSpecimens.add(specimenId);
  if (phoneSpecimens.size > 1) return;
  const native = window.matchMedia.bind(window);
  window.matchMedia = (query: string) => (query === SHELL_NARROW_QUERY && phoneRoute() ? forcedMediaList(query) : native(query));
}

export function PhoneViewport({ children }: { children: ReactNode }) {
  useEffect(() => {
    if (phoneMediaMirrored) return;
    phoneMediaMirrored = true;
    for (const sheet of Array.from(document.styleSheets)) {
      try {
        mirrorPhoneRules(sheet.cssRules, sheet);
      } catch {
        // Cross-origin sheets are not readable.
      }
    }
  }, []);
  return <div className={PHONE_SCOPE}>{children}</div>;
}

/** Clicks the first element matching `selector` once it renders, so a disclosure opens through its own handler. */
export function ClickOnce({ selector, children }: { selector: string; children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    let alive = true;
    void waitFor(() => ref.current?.querySelector<HTMLElement>(selector), 240).then((target) => {
      if (alive) target?.click();
    });
    return () => {
      alive = false;
    };
  }, [selector]);
  return <div ref={ref} className="gallery-fill">{children}</div>;
}
