import { useEffect, useRef, type ReactNode } from "react";

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

/** Applies a forced pseudo-state class to the first element matching `selector` inside the children. */
export function ForcePseudo({ selector, state, children }: { selector: string; state: PseudoState; children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    mirrorPseudoRules();
    const cls = PSEUDO.find(([pseudo]) => pseudo === `:${state}`)?.[1];
    const apply = () => {
      const target = ref.current?.querySelector(selector);
      if (cls && target && !target.classList.contains(cls)) target.classList.add(cls);
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

async function waitFor<T>(read: () => T | null | undefined, attempts = 60): Promise<T | null> {
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
