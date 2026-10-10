const noop = async () => undefined;
const unlisten = async () => () => undefined;

const currentWindow = {
  label: "main",
  close: noop,
  destroy: noop,
  minimize: noop,
  maximize: noop,
  unmaximize: noop,
  toggleMaximize: noop,
  isMaximized: async () => false,
  isFullscreen: async () => false,
  setFocus: noop,
  startDragging: noop,
  onResized: unlisten,
  onFocusChanged: unlisten,
  onCloseRequested: unlisten,
  listen: unlisten,
};

export function getCurrentWindow() {
  return currentWindow;
}
