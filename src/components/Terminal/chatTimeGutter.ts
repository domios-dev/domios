import { showChatTimeGutter } from "@codeterm/chat-ui";

export function syncChatTimeGutter(surface: HTMLElement, width: number): void {
  const show = showChatTimeGutter(width);
  surface.dataset.timeGutter = String(show);
  for (const time of surface.querySelectorAll<HTMLElement>("[data-testid='chat-message-time']")) time.hidden = !show;
}
