import type { Tab } from "../../lib/types";
import { createKeyedFlagStore } from "../../lib/keyedFlagStore";
import { isSurfaceTab, surfaceOwnerId } from "../../lib/surfaceTab";

/** Owner rows whose surfaces are folded away, keyed by owner tab id. */
export const attachmentCollapseStore = createKeyedFlagStore();

/** Width of a row's avatar slot: an attachment sits one avatar in from its owner. */
export const ATTACHMENT_AVATAR_INDENT = 14;
const LOOSE_ROW_INSET = 16;
const LOOSE_ROW_TOUCH_INSET = 20;
const TEAM_ROW_INSET = 12;

export type OwnerRowContext = "loose" | "team";

export function ownerRowInset(context: OwnerRowContext, touchMode: boolean): number {
  if (context === "team") return TEAM_ROW_INSET;
  return touchMode ? LOOSE_ROW_TOUCH_INSET : LOOSE_ROW_INSET;
}

export function attachmentInset(ownerInset: number): number {
  return ownerInset + ATTACHMENT_AVATAR_INDENT;
}

/** Surfaces grouped under an owner present in `tabs`, in tab order; a surface never owns attachments. */
export function surfaceAttachmentsByOwner(tabs: readonly Tab[]): Map<string, Tab[]> {
  const byId = new Map(tabs.map((tab) => [tab.id, tab]));
  const grouped = new Map<string, Tab[]>();
  for (const tab of tabs) {
    if (!isSurfaceTab(tab)) continue;
    const ownerId = surfaceOwnerId(tab);
    const owner = ownerId ? byId.get(ownerId) : undefined;
    if (!owner || isSurfaceTab(owner)) continue;
    grouped.set(owner.id, [...(grouped.get(owner.id) ?? []), tab]);
  }
  return grouped;
}

export interface AttachmentBadge {
  /** Shown beside the globe only when more than one surface hides behind it. */
  count: number | null;
  label: string;
}

export function collapsedAttachmentBadge(attachmentCount: number, collapsed: boolean): AttachmentBadge | null {
  if (!collapsed || attachmentCount <= 0) return null;
  return {
    count: attachmentCount > 1 ? attachmentCount : null,
    label: attachmentCount === 1 ? "1 surface" : `${attachmentCount} surfaces`,
  };
}

export interface AttachmentBlockModel<T> {
  rows: readonly T[];
  badge: AttachmentBadge | null;
  tethered: boolean;
}

/** Collapsed owners hide their rows; the compact sidebar keeps rows but swaps the tether for a badge. */
export function attachmentBlockModel<T>(attachments: readonly T[], collapsed: boolean, compact: boolean): AttachmentBlockModel<T> {
  const rows = collapsed ? [] : attachments;
  return {
    rows,
    badge: collapsedAttachmentBadge(attachments.length, collapsed),
    tethered: !compact && rows.length > 0,
  };
}
