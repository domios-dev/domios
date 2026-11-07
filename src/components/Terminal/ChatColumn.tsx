import type { HTMLAttributes } from "react";

/** Lines a live turn block up with the transcript's readable column on every surface width. */
export function ChatColumn({ children, ...rest }: Omit<HTMLAttributes<HTMLDivElement>, "className">) {
  return (
    <div {...rest} className="ct-chat-message-row ct-chat-message-row--agent w-full max-w-4xl min-w-0 mx-auto">
      {children}
    </div>
  );
}
