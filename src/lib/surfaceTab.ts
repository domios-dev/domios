import type { SurfaceTabBacking, Tab, TabBacking } from "./types";

export type TabBody = "surface_view" | "chat_backend" | "terminal";

export function surfaceBacking(tab: { backing?: TabBacking } | null | undefined): SurfaceTabBacking | null {
  return tab?.backing?.kind === "surface" ? tab.backing : null;
}

export function isSurfaceTab(tab: { backing?: TabBacking } | null | undefined): boolean {
  return surfaceBacking(tab) !== null;
}

/** A tab without a backing predates the field and is a PTY. */
export function isPtyBacked(tab: { backing?: TabBacking }): boolean {
  return tab.backing == null || tab.backing.kind === "pty";
}

export function tabBody(tab: { backing?: TabBacking }): TabBody {
  switch (tab.backing?.kind) {
    case "surface":
      return "surface_view";
    case "chat_backend":
      return "chat_backend";
    default:
      return "terminal";
  }
}

/** Surface tabs take no text: actions go through the plugin's verbs. */
export function canSendText(tab: { backing?: TabBacking }): boolean {
  return !isSurfaceTab(tab);
}

/** "{Owner}'s {Title}" when the surface asks for its owner's name and the owner is known. */
export function surfaceDisplayLabel(title: string, backing: SurfaceTabBacking, ownerName: string | null | undefined): string {
  const owner = ownerName?.trim();
  return backing.owner_label && owner ? `${owner}'s ${title}` : title;
}

export function surfaceOwnerId(tab: Pick<Tab, "parent_tab_id">): string | null {
  return tab.parent_tab_id ?? null;
}

/** Identity a surface tab hands its plugin view. */
export interface PluginViewSurface {
  surfaceId: string;
  instanceId: string;
  tabId: string;
}

export function pluginViewSurface(tabId: string, backing: SurfaceTabBacking): PluginViewSurface {
  return { surfaceId: backing.surface_id, instanceId: backing.instance_id, tabId };
}

/** Query suffix for the view URL; the sandboxed page reads it from `location.search`. */
export function surfaceQuery(surface: PluginViewSurface | undefined): string {
  if (!surface) return "";
  const params = new URLSearchParams({ surface: surface.surfaceId, instance: surface.instanceId, tab: surface.tabId });
  return `&${params.toString()}`;
}

export function workspaceIdOfTab(workspaces: readonly { id: string; tabs: readonly string[] }[], tabId: string | null | undefined): string | null {
  if (!tabId) return null;
  return workspaces.find((workspace) => workspace.tabs.includes(tabId))?.id ?? null;
}

/** The owner's title a sidebar row composes into its label; null when the row is not an owner-labelled surface. */
export function surfaceOwnerTitle(tab: Tab, tabs: Readonly<Record<string, Pick<Tab, "title">>>): string | null {
  const backing = surfaceBacking(tab);
  if (!backing?.owner_label || !tab.parent_tab_id) return null;
  return tabs[tab.parent_tab_id]?.title ?? null;
}

/** The label a tab row shows: surfaces compose their owner's name, every other tab keeps its title. */
export function tabRowLabel(tab: Tab, title: string, ownerTitle: string | null | undefined): string {
  const backing = surfaceBacking(tab);
  return backing ? surfaceDisplayLabel(title, backing, ownerTitle) : title;
}
