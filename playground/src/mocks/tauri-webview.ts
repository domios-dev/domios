const currentWebview = {
  label: "main",
  onDragDropEvent: async () => () => undefined,
  setZoom: async () => undefined,
};

export function getCurrentWebview() {
  return currentWebview;
}
