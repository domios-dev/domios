import React from "react";
import { TEXT_WRAP_CLASS } from "./MarkdownBody";

export const CROSS_TAB_BUBBLE_TESTID = "chat-cross-tab-bubble";
export const CROSS_TAB_SHELL_TESTID = "chat-cross-tab-shell";

export function crossTabBubbleClassName(textSize: string): string {
  return `min-w-0 rounded-2xl rounded-br-sm px-4 py-2 ${textSize} select-text cursor-text ${TEXT_WRAP_CLASS}`;
}

export const crossTabBubbleStyle: React.CSSProperties = {
  background: "color-mix(in srgb, var(--ct-accent, #58a6ff) 12%, var(--ct-surface, #161b22) 88%)",
  color: "var(--ct-fg)",
};

interface CrossTabMessageShellProps {
  header: React.ReactNode;
  textSize: string;
  bubbleTestId?: string;
  footer?: React.ReactNode;
  afterBubble?: React.ReactNode;
  children: React.ReactNode;
}

export function CrossTabMessageShell({
  header,
  textSize,
  bubbleTestId = CROSS_TAB_BUBBLE_TESTID,
  footer,
  afterBubble,
  children,
}: CrossTabMessageShellProps) {
  return (
    <div
      data-testid={CROSS_TAB_SHELL_TESTID}
      className="group flex mb-3 w-full max-w-4xl min-w-0 mx-auto justify-start"
    >
      <div className="min-w-0 max-w-[75%] sm:max-w-[75%]">
        {header}
        <div
          data-testid={bubbleTestId}
          className={crossTabBubbleClassName(textSize)}
          style={crossTabBubbleStyle}
        >
          {children}
        </div>
        {afterBubble}
        {footer}
      </div>
    </div>
  );
}
