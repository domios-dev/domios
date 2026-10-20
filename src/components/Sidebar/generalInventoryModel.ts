export function inventoryGroupVisibility(count: number, expanded: boolean, hasShowAll: boolean) {
  const visible = count > 0;
  const open = visible && expanded;
  return { visible, open, showAll: open && hasShowAll };
}
