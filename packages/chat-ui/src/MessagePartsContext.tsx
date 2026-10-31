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
  const part = typeof id === "string" && context ? { context, id } : null;
  const selected = part?.context.selected?.(part.id);
  const attributes = part ? {
    role: "group", "aria-label": "Message part", tabIndex: part.context.cursor?.(part.id) ? 0 : -1,
    "data-selected": selected || undefined, "data-preview": part.context.preview === part.id || undefined,
  } : {};
  return <div {...props} {...attributes}>
    {part?.context.selecting && <span className="ct-part-gutter" data-message-gutter="" aria-hidden="true">
      <span className="ct-msg-bead" data-selected={selected || undefined} />
      {part.context.joinNext?.(part.id) && <span className="ct-part-thread" />}
    </span>}
    {props.children}
  </div>;
}
