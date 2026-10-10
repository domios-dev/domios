import { createContext, useContext, type ComponentProps } from "react";

export const MessagePartsContext = createContext<{
  id: string;
  source: string;
  selected?: (id: string) => boolean;
  cursor?: (id: string) => boolean;
  selecting?: boolean;
  joinNext?: (id: string) => boolean;
  preview?: string | null;
} | null>(null);

export function MessagePartView({ node: _node, ...props }: ComponentProps<"div"> & { node?: unknown }) {
  const context = useContext(MessagePartsContext);
  const id = (props as Record<string, unknown>)["data-message-part-id"];
  if (typeof id !== "string" || !context) return <div {...props} />;
  const selected = context.selected?.(id);
  return <div {...props} role="group" aria-label="Message part" tabIndex={context.cursor?.(id) ? 0 : -1}
    data-selected={selected || undefined} data-preview={context.preview === id || undefined}>
    {context.selecting && <span className="ct-part-gutter" data-message-gutter="" aria-hidden="true">
      <span className="ct-msg-bead" data-selected={selected || undefined} />
      {context.joinNext?.(id) && <span className="ct-part-thread" />}
    </span>}
    {props.children}
  </div>;
}
