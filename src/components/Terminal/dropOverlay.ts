export function shouldShowDropOverlay({
  isFileDragOver,
  isTextDragOver,
  termReady,
}: {
  isFileDragOver: boolean;
  isTextDragOver: boolean;
  termReady: boolean;
}): boolean {
  return (isFileDragOver || isTextDragOver) && termReady;
}
