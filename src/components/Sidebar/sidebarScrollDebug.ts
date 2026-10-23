import { DEV_BUILD } from "../../lib/devBuild";

export function sidebarScrollDebug(viewport: HTMLElement, edge: string, detail: object = {}): void {
  if (!DEV_BUILD) return;
  viewport.dispatchEvent(new CustomEvent("sidebar-scroll-debug", {
    bubbles: true,
    detail: { edge, scrollTop: viewport.scrollTop, ...detail },
  }));
}

export function observeSidebarRows(viewport: HTMLElement, body: HTMLElement): () => void {
  if (!DEV_BUILD) return () => {};
  const observer = new MutationObserver((records) => {
    for (const record of records) {
      for (const [edge, nodes] of [["row-added", record.addedNodes], ["row-removed", record.removedNodes]] as const) {
        for (const node of nodes) {
          if (!(node instanceof Element)) continue;
          const rows = [...node.querySelectorAll<HTMLElement>("[data-tab-id]")];
          if (node.matches("[data-tab-id]")) rows.unshift(node as HTMLElement);
          for (const row of rows) sidebarScrollDebug(viewport, edge, { tabId: row.dataset.tabId });
        }
      }
    }
  });
  observer.observe(body, { childList: true, subtree: true });
  return () => observer.disconnect();
}
