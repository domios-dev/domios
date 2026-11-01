import React from "react";
import { TEXT_WRAP_CLASS } from "./MarkdownBody";

export const CROSS_TAB_BUBBLE_TESTID = "chat-cross-tab-bubble";
export const CROSS_TAB_SHELL_TESTID = "chat-cross-tab-shell";

export function crossTabBubbleClassName(textSize: string): string {
  return `min-w-0 py-1 ${textSize} select-text cursor-text ${TEXT_WRAP_CLASS}`;
}

export const crossTabBubbleStyle: React.CSSProperties = {
  background: "transparent",
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
      className="group flex w-full min-w-0 justify-start"
    >
      <div className="min-w-0 w-full">
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
