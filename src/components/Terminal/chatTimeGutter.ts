import { chatHorizontalInsets } from "@codeterm/chat-ui";

export function syncChatTimeGutter(surface: HTMLElement, width: number, composerInset: number, timestampWidth: number): void {
  const insets = chatHorizontalInsets(width, composerInset, timestampWidth);
  const show = insets.timeGutter > 0;
  surface.style.setProperty("--ct-chat-content-inset", `${insets.contentInset}px`);
  surface.style.setProperty("--ct-chat-composer-inset", `${insets.composerInset}px`);
  surface.dataset.timeGutter = String(show);
  for (const time of surface.querySelectorAll<HTMLElement>("[data-testid='chat-message-time']")) time.hidden = !show;
}
