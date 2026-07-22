import { expect, type Locator, type Page } from "@playwright/test";

export const OPTIONAL_SURFACES = [
  '[data-testid="new-session-picker"]',
  '[data-testid="settings-panel"]',
  '[data-testid="mesh-tab-network"]',
  '[data-testid="peer-picker-menu"]',
  '[data-testid="settings-menu"]',
  '[data-testid="sidebar"]',
] as const;

export const INTERACTIVE = [
  "button",
  "a[href]",
  "input",
  "select",
  "textarea",
  '[role="button"]',
  '[role="menuitem"]',
  '[role="tab"]',
  '[role="switch"]',
  '[tabindex]:not([tabindex="-1"])',
].join(",");

export type ReachabilityMode = "all" | "open-drawer" | "peer-picker-menu" | "settings-menu" | "workspace-dropdown" | "new-session-picker";

export const SANCTIONED_HSCROLL_OWNER = {
  className: "ct-hscroll",
  overflowX: ["auto", "scroll"],
} as const;

export async function settleLayout(page: Page): Promise<void> {
  await page.evaluate(() => new Promise<void>((resolve) => {
    requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
  }));
}

export async function collectViewportOffenders(page: Page, reachabilityMode: ReachabilityMode = "all") {
  return page.locator(INTERACTIVE).evaluateAll((nodes, { mode, hscroll }) => {
    const viewportWidth = document.documentElement.clientWidth;
    const viewportHeight = document.documentElement.clientHeight;
    const openDrawer = mode === "open-drawer"
      ? document.querySelector(".ct-sidebar-drawer.is-open")
      : null;
    const openBackdrop = mode === "open-drawer"
      ? document.querySelector(".ct-sidebar-backdrop.is-open")
      : null;
    const peerPickerMenu = mode === "peer-picker-menu"
      ? document.querySelector('[data-testid="peer-picker-menu"]')
      : null;
    const settingsMenu = mode === "settings-menu"
      ? document.querySelector('[data-testid="settings-menu"]')
      : null;
    const newSessionDialog = mode === "new-session-picker" || mode === "workspace-dropdown"
      ? document.querySelector('[data-testid="new-session-picker"] > [role="dialog"][aria-modal="true"]')
      : null;
    const workspaceOption = mode === "workspace-dropdown"
      ? Array.from(document.querySelectorAll('[data-testid^="picker-workspace-option-"]')).find((node) => {
        const rect = node.getBoundingClientRect();
        const style = window.getComputedStyle(node);
        return style.display !== "none" && style.visibility !== "hidden" && rect.width > 0 && rect.height > 0;
      })
      : null;
    const workspaceFloat = workspaceOption?.closest(".ct-float") as HTMLElement | null;
    const workspaceDropdown = workspaceFloat && window.getComputedStyle(workspaceFloat).position === "fixed" ? workspaceFloat : null;
    const clips = (value: string) => /auto|scroll|hidden|clip/.test(value);
    const isFullyClippedByAncestor = (el: HTMLElement, rect: DOMRect) => {
      let parent = el.parentElement;
      while (parent && parent !== document.body && parent !== document.documentElement) {
        if (parent.id === "root") {
          parent = parent.parentElement;
          continue;
        }
        const style = window.getComputedStyle(parent);
        const clipsX = clips(style.overflowX);
        const clipsY = clips(style.overflowY);
        if (clipsX || clipsY) {
          const clipRect = parent.getBoundingClientRect();
          const clippedX = rect.right <= clipRect.left || rect.left >= clipRect.right;
          const clippedY = rect.bottom <= clipRect.top || rect.top >= clipRect.bottom;
          if ((clipsX && clippedX) || (clipsY && clippedY)) return true;
        }
        parent = parent.parentElement;
      }
      return false;
    };
    const hasInactiveSemantics = (el: HTMLElement) => {
      let current: HTMLElement | null = el;
      while (current && current !== document.body && current !== document.documentElement) {
        if (current.hasAttribute("inert") || current.getAttribute("aria-hidden") === "true") return true;
        current = current.parentElement;
      }
      return false;
    };
    const ownsSanctionedHorizontalClip = (el: HTMLElement, rect: DOMRect) => {
      const owner = el.parentElement?.closest(`.${hscroll.className}`) as HTMLElement | null;
      if (!owner || owner === el || !owner.contains(el)) return false;
      const ownerRect = owner.getBoundingClientRect();
      const overflowX = window.getComputedStyle(owner).overflowX;
      const partlyInViewport = rect.right > 0 && rect.left < viewportWidth;
      const partlyInOwner = rect.right > ownerRect.left && rect.left < ownerRect.right;
      return partlyInViewport &&
        partlyInOwner &&
        ownerRect.left >= -1 &&
        ownerRect.right <= viewportWidth + 1 &&
        ownerRect.top >= -1 &&
        ownerRect.bottom <= viewportHeight + 1 &&
        hscroll.overflowX.includes(overflowX as "auto" | "scroll");
    };
    return nodes
      .map((node) => {
        const el = node as HTMLElement;
        const style = window.getComputedStyle(el);
        const rect = el.getBoundingClientRect();
        const hidden =
          style.visibility === "hidden" ||
          style.display === "none" ||
          rect.width <= 0 ||
          rect.height <= 0 ||
          isFullyClippedByAncestor(el, rect) ||
          hasInactiveSemantics(el) ||
          el.closest(".xterm-accessibility, .xterm-helper-textarea") !== null;
        if (hidden) return null;
        const inNewSessionDialog = !!newSessionDialog?.contains(el);
        const inWorkspaceDropdown = !!workspaceDropdown?.contains(el);
        if (mode === "new-session-picker" && !inNewSessionDialog) return null;
        if (mode === "workspace-dropdown" && !inNewSessionDialog && !inWorkspaceDropdown) return null;
        const left = rect.left;
        const right = rect.right;
        const top = rect.top;
        const bottom = rect.bottom;
        const centerX = Math.min(Math.max(left + rect.width / 2, 0), viewportWidth - 1);
        const centerY = Math.min(Math.max(top + rect.height / 2, 0), viewportHeight - 1);
        const hit = document.elementFromPoint(centerX, centerY);
        const reachable = hit instanceof Node && (hit === el || el.contains(hit) || hit.contains(el));
        const inOpenDrawer = !!openDrawer?.contains(el);
        const hitInOpenLayer = hit instanceof Node &&
          (!!openDrawer?.contains(hit) || hit === openDrawer || !!openBackdrop?.contains(hit) || hit === openBackdrop);
        const inPeerPickerMenu = !!peerPickerMenu?.contains(el);
        const hitInPeerPickerMenu = hit instanceof Node &&
          (!!peerPickerMenu?.contains(hit) || hit === peerPickerMenu);
        const inSettingsMenu = !!settingsMenu?.contains(el);
        const hitInSettingsMenu = hit instanceof Node &&
          (!!settingsMenu?.contains(hit) || hit === settingsMenu);
        const hitInWorkspaceDropdown = hit instanceof Node &&
          (!!workspaceDropdown?.contains(hit) || hit === workspaceDropdown);
        const xInBounds = left >= -1 && right <= viewportWidth + 1;
        const yInBounds = top >= -1 && bottom <= viewportHeight + 1;
        const inBounds = (xInBounds || ownsSanctionedHorizontalClip(el, rect)) && yInBounds;
        let layerReachabilityOk = reachable;
        if (mode === "open-drawer") {
          layerReachabilityOk = inOpenDrawer ? reachable : !reachable && hitInOpenLayer;
        } else if (mode === "peer-picker-menu") {
          layerReachabilityOk = inPeerPickerMenu ? reachable : reachable || (!reachable && hitInPeerPickerMenu);
        } else if (mode === "settings-menu") {
          layerReachabilityOk = inSettingsMenu ? reachable : reachable || (!reachable && hitInSettingsMenu);
        } else if (mode === "workspace-dropdown") {
          layerReachabilityOk = inWorkspaceDropdown ? reachable : reachable || (!reachable && hitInWorkspaceDropdown);
        }
        if (inBounds && layerReachabilityOk) return null;
        return {
          tag: el.tagName.toLowerCase(),
          testid: el.getAttribute("data-testid"),
          role: el.getAttribute("role"),
          title: el.getAttribute("title"),
          text: (el.innerText || el.textContent || "").trim().slice(0, 80),
          left,
          top,
          right,
          bottom,
          width: rect.width,
          height: rect.height,
          viewportWidth,
          viewportHeight,
          reachable,
          inOpenDrawer,
          hitInOpenLayer,
          inPeerPickerMenu,
          hitInPeerPickerMenu,
          inSettingsMenu,
          hitInSettingsMenu,
          inNewSessionDialog,
          inWorkspaceDropdown,
          hitInWorkspaceDropdown,
          hitTag: hit instanceof HTMLElement ? hit.tagName.toLowerCase() : null,
          hitTestid: hit instanceof HTMLElement ? hit.getAttribute("data-testid") : null,
        };
      })
      .filter(Boolean);
  }, { mode: reachabilityMode, hscroll: SANCTIONED_HSCROLL_OWNER });
}

export async function revealOwnedHorizontalSurface(page: Page, loc: Locator, label: string): Promise<void> {
  const owner = loc.locator(`xpath=ancestor::*[contains(concat(' ', normalize-space(@class), ' '), ' ${SANCTIONED_HSCROLL_OWNER.className} ')]`).first();
  if (!await owner.count()) return;
  const metrics = await owner.evaluate((el) => {
    const rect = el.getBoundingClientRect();
    return { left: rect.left, right: rect.right, width: rect.width, viewport: document.documentElement.clientWidth, overflowX: window.getComputedStyle(el).overflowX };
  });
  expect(metrics.left, `${label} hscroll owner left`).toBeGreaterThanOrEqual(-1);
  expect(metrics.right, `${label} hscroll owner right`).toBeLessThanOrEqual(metrics.viewport + 1);
  expect(metrics.overflowX, `${label} hscroll owner overflow-x`).toMatch(new RegExp(`^(${SANCTIONED_HSCROLL_OWNER.overflowX.join("|")})$`));
  await loc.scrollIntoViewIfNeeded();
  await settleLayout(page);
}

export async function auditKnownSurfaces(page: Page, label: string): Promise<void> {
  for (const selector of OPTIONAL_SURFACES) {
    const loc = page.locator(selector).first();
    if (!(await loc.isVisible().catch(() => false))) continue;
    const inactive = await loc.evaluate((el) => el.closest('[inert], [aria-hidden="true"]') !== null);
    if (inactive) continue;
    await revealOwnedHorizontalSurface(page, loc, `${label} ${selector}`);
    const box = await loc.boundingBox();
    if (!box) continue;
    expect(box.x, `${label} ${selector} left`).toBeGreaterThanOrEqual(-1);
    expect(box.x + box.width, `${label} ${selector} right`).toBeLessThanOrEqual(page.viewportSize()!.width + 1);
    expect(box.y, `${label} ${selector} top`).toBeGreaterThanOrEqual(-1);
    expect(box.y + box.height, `${label} ${selector} bottom`).toBeLessThanOrEqual(page.viewportSize()!.height + 1);
  }
}
