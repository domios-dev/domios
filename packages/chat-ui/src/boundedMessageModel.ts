export function expandedMessageHeight(collapsedHeight: number): number {
  return Number((collapsedHeight * 1.3).toFixed(4));
}

export function boundedMessageLayout(collapsedHeight: string, expanded: boolean) {
  const dimension = /^(\d+(?:\.\d+)?)(px|rem|em|vh|dvh)$/.exec(collapsedHeight);
  const maxHeight = !expanded ? collapsedHeight : dimension
    ? `${expandedMessageHeight(Number(dimension[1]))}${dimension[2]}`
    : `calc(${collapsedHeight} * 1.3)`;
  return { maxHeight, overflowY: expanded ? "auto" as const : "hidden" as const };
}

export function boundedMessageIsClipped(scrollHeight: number, clientHeight: number): boolean {
  return clientHeight > 0 && scrollHeight > clientHeight + 1;
}
