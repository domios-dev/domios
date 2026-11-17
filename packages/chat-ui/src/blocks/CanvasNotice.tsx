import type { ReactNode } from "react";

export function CanvasNotice({ text, reason }: { text: string; reason?: string }) {
  return (
    <div role="note" data-testid="chat-ds-canvas" data-canvas-invalid="" className="ct-card ct-card--quiet my-2 w-fit max-w-[34rem] px-3 py-2 text-xs" style={{ color: "var(--ct-muted-fg)" }}>
      <CanvasNoticeBody text={text} reason={reason} />
    </div>
  );
}

/** The canvas notice pattern: one quiet sentence, an optional action, and the cause under Details. */
export function CanvasNoticeBody({ text, reason, action }: { text: string; reason?: string; action?: ReactNode }) {
  return <>
    <p className="m-0 flex flex-wrap items-center gap-x-1">{text}{action}</p>
    {reason && <details className="mt-1 text-[11px] opacity-80"><summary className="cursor-pointer select-none">Details</summary><p className="m-0 mt-1 break-words">{reason}</p></details>}
  </>;
}
