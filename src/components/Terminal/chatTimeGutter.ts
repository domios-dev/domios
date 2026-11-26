import { chatHorizontalInsets } from "@codeterm/chat-ui";

export function syncChatTimeGutter(surface: HTMLElement, width: number, composerInset: number, timestampWidth: number, columnWidth: number): void {
  const insets = chatHorizontalInsets(width, composerInset, timestampWidth, columnWidth);
  const show = insets.timeGutter > 0;
  const contentValue = `${insets.contentInset}px`;
  const composerValue = `${insets.composerInset}px`;
  if (surface.style.getPropertyValue("--ct-chat-content-inset") !== contentValue) {
    surface.style.setProperty("--ct-chat-content-inset", contentValue);
  }
  if (surface.style.getPropertyValue("--ct-chat-composer-inset") !== composerValue) {
    surface.style.setProperty("--ct-chat-composer-inset", composerValue);
  }
  const showAttr = String(show);
  if (surface.dataset.timeGutter !== showAttr) surface.dataset.timeGutter = showAttr;
  for (const time of surface.querySelectorAll<HTMLElement>("[data-testid='chat-message-time']")) {
    if (time.hidden !== !show) time.hidden = !show;
  }
}
