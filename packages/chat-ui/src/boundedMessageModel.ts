export function boundedMessageLayout(collapsedHeight: string, expanded: boolean) {
  return { maxHeight: expanded ? "none" : collapsedHeight, overflowY: expanded ? "auto" as const : "hidden" as const, overflowX: "auto" as const };
}

export function boundedMessageIsClipped(scrollHeight: number, clientHeight: number): boolean {
  return clientHeight > 0 && scrollHeight > clientHeight + 1;
}
