import type { ReactNode } from "react";
import { composerVisible, type ChatBodyView } from "./chatBodyVisibility";

export function ChatComposerSurface({ view, children }: { view: ChatBodyView; children: ReactNode }) {
  return composerVisible(view) ? <div data-composer-height-surface className="ct-chat-bottom-bar shrink-0">{children}</div> : null;
}
