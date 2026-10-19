import { useLayoutEffect } from "react";
import { ActivityDots } from "../shared/ActivityDots";

export function ChatWorkingRow({ onContentLayout }: { onContentLayout: () => void }) {
  useLayoutEffect(onContentLayout, [onContentLayout]);
  return (
    <div
      data-testid="chat-working-row"
      className="mx-auto mb-1.5 flex w-full max-w-4xl min-w-0 items-center gap-2 px-3 py-2 text-xs"
      style={{ color: "var(--ct-muted-fg)" }}
    >
      <ActivityDots size="small" tone="accent" rhythm="work" active />
      <span>Working…</span>
    </div>
  );
}
