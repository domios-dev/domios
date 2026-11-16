import { MACHINE_CARD_WIDTH } from "./webGraphLayout";

/** Below this width a touch screen is a phone: the inventory offers its stacked list. */
export const PHONE_MAX_WIDTH = 640;
/** The list's side gutters, so a card never touches the screen edge. */
const LIST_GUTTER = 24;

export type InventoryView = "list" | "graph";

/**
 * At phone width the radial graph of 360 px cards only fits at about a fifth of
 * its size, so a touch phone gets the stacked list of full-size cards first and
 * keeps the graph one tap away. Desktop, at any width, keeps the graph alone.
 */
export function offersInventoryList(width: number, touch: boolean): boolean {
  return touch && width > 0 && width < PHONE_MAX_WIDTH;
}

/** The view the inventory shows: the list on a touch phone unless the user chose the graph. */
export function inventoryView(width: number, touch: boolean, chosen: InventoryView | null): InventoryView {
  return offersInventoryList(width, touch) ? chosen ?? "list" : "graph";
}

/** How much a list card shrinks so it fits the screen with its gutters; never enlarged. */
export function listCardScale(width: number): number {
  return width <= 0 ? 1 : Math.min(1, (width - LIST_GUTTER) / MACHINE_CARD_WIDTH);
}
