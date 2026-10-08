import { memo, useState, type MouseEvent, type ReactNode } from "react";
import { Check, Circle, CircleCheck, Copy, Hash, Quote } from "lucide-react";
import { messageClipboard, messageMarkdownClipboard, sourceDiffersFromVisible, type ClipMessage } from "@codeterm/chat-core";
import { writeClipboardFlavours } from "@codeterm/chat-ui";

export interface MessageRowHandlers {
  clipFor: (id: string) => ClipMessage | null;
  toggle: (id: string) => void;
  rowMouseDown: (id: string, event: MouseEvent<HTMLElement>) => void;
  rowClick: (id: string, event: MouseEvent<HTMLElement>) => void;
  quote: (clip: ClipMessage) => void;
}

function TrayButton({ label, onClick, children, pressed, ...rest }: {
  label: string;
  onClick: () => void;
  children: ReactNode;
  pressed?: boolean;
  [data: `data-${string}`]: string | boolean | undefined;
}) {
  return (
    <button
      type="button"
      className="ct-msg-actions__button"
      aria-label={label}
      title={label}
      {...(pressed === undefined ? {} : { role: "checkbox", "aria-checked": pressed })}
      onClick={(event) => { event.stopPropagation(); onClick(); }}
      {...rest}
    >
      {children}
    </button>
  );
}

function MessageActions({ id, selected, handlers }: { id: string; selected: boolean; handlers: MessageRowHandlers }) {
  const clip = handlers.clipFor(id);
  const [copied, setCopied] = useState<"copy" | "markdown" | null>(null);
  if (!clip) return null;
  const copy = (kind: "copy" | "markdown") => {
    writeClipboardFlavours(kind === "copy" ? messageClipboard(clip) : messageMarkdownClipboard(clip));
    setCopied(kind);
  };
  return (
    <div role="toolbar" aria-label="Message actions" className="ct-msg-actions" onMouseLeave={() => setCopied(null)}>
      <TrayButton label="Select message" pressed={selected} data-message-select onClick={() => handlers.toggle(id)}>
        {selected ? <CircleCheck size={13} aria-hidden /> : <Circle size={13} aria-hidden />}
      </TrayButton>
      <TrayButton label={copied === "copy" ? "Copied" : "Copy message"} onClick={() => copy("copy")}>
        {copied === "copy" ? <Check size={13} aria-hidden /> : <Copy size={13} aria-hidden />}
      </TrayButton>
      {sourceDiffersFromVisible(clip) && (
        <TrayButton label={copied === "markdown" ? "Copied as Markdown" : "Copy as Markdown"} onClick={() => copy("markdown")}>
          {copied === "markdown" ? <Check size={13} aria-hidden /> : <Hash size={13} aria-hidden />}
        </TrayButton>
      )}
      <TrayButton label="Quote into composer" onClick={() => handlers.quote(clip)}>
        <Quote size={13} aria-hidden />
      </TrayButton>
    </div>
  );
}

/** One chat row: the bubble, its quiet action tray on hover or focus, and its place in a message selection. */
export const ChatMessageRow = memo(function ChatMessageRow({ msg, ariaLabel, selected, joinNext, selecting, cursor, handlers, children }: {
  msg: { id: string; type: string; sourceMessageId?: string };
  ariaLabel: string;
  selected: boolean;
  joinNext: boolean;
  selecting: boolean;
  cursor: boolean;
  handlers: MessageRowHandlers;
  children: ReactNode;
}) {
  const [hot, setHot] = useState(false);
  return (
    <div
      data-testid="chat-message"
      data-message-role={msg.type}
      data-message-id={msg.id}
      data-source-message-id={msg.sourceMessageId ?? msg.id}
      data-selected={selected || undefined}
      data-join-next={(selected && joinNext) || undefined}
      data-selecting={selecting || undefined}
      role="article"
      aria-label={ariaLabel}
      tabIndex={cursor ? 0 : -1}
      className="ct-msg-row"
      onPointerEnter={() => setHot(true)}
      onPointerLeave={() => setHot(false)}
      onFocus={() => setHot(true)}
      onBlur={(event) => { if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setHot(false); }}
      onMouseDown={(event) => handlers.rowMouseDown(msg.id, event)}
      onClick={(event) => handlers.rowClick(msg.id, event)}
    >
      {children}
      {hot && !selecting && (
        <div className="ct-msg-row__tray-lane">
          <MessageActions id={msg.id} selected={selected} handlers={handlers} />
        </div>
      )}
    </div>
  );
});
