export function composerMentionDropTarget(point: { x: number; y: number }, editors: Iterable<HTMLTextAreaElement>): HTMLTextAreaElement | null {
  for (const editor of editors) {
    const rect = editor.getBoundingClientRect();
    if (!editor.disabled && rect.width > 0 && rect.height > 0 && point.x >= rect.left && point.x <= rect.right && point.y >= rect.top && point.y <= rect.bottom) return editor;
  }
  return null;
}

export function trackComposerMentionDrop() {
  let point: { x: number; y: number } | null = null;
  const mouse = (event: MouseEvent) => { point = { x: event.clientX, y: event.clientY }; };
  const touch = (event: TouchEvent) => {
    const item = event.touches[0] ?? event.changedTouches[0];
    if (item) point = { x: item.clientX, y: item.clientY };
  };
  document.addEventListener("mousemove", mouse);
  document.addEventListener("touchmove", touch);
  document.addEventListener("mouseup", mouse, true);
  document.addEventListener("touchend", touch, true);
  return (tabId?: string) => {
    document.removeEventListener("mousemove", mouse);
    document.removeEventListener("touchmove", touch);
    document.removeEventListener("mouseup", mouse, true);
    document.removeEventListener("touchend", touch, true);
    if (!tabId || !point) return false;
    const editor = composerMentionDropTarget(point, document.querySelectorAll<HTMLTextAreaElement>('textarea[data-testid="chat-composer"]'));
    if (!editor) return false;
    editor.dispatchEvent(new CustomEvent("domios-insert-tab-mention", { detail: { id: tabId } }));
    return true;
  };
}
