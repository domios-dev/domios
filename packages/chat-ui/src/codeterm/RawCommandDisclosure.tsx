import { ExpandableReader } from "../ExpandableReader";
import { BoundedMessageContent } from "../BoundedMessageContent";
import React from "react";
import { ChevronRight } from "lucide-react";

interface Props {
  raw: string;
  label?: string;
  children?: React.ReactNode;
  /** Shown with the raw command, and hidden with it. */
  details?: React.ReactNode;
  initiallyExpanded?: boolean;
}

export function RawCommandDisclosure({ raw, children, details, initiallyExpanded = false, label = "Raw command" }: Props) {
  const [open, setOpen] = React.useState(initiallyExpanded);
  const detailsId = React.useId();

  return (
    <ExpandableReader label={label}>{(fullscreen) => (
    <div data-testid="raw-command-disclosure" className="grid gap-0.5">
      <button
        type="button"
        aria-expanded={open}
        aria-controls={detailsId}
        onClick={() => setOpen((value) => !value)}
        className="inline-flex w-fit items-center gap-1 py-0 font-mono text-[10.5px] transition-colors hover:brightness-125"
        style={{ color: "var(--ct-muted-fg)", background: "transparent" }}
      >
        <ChevronRight aria-hidden="true" size={12} strokeWidth={1.8} className={open ? "rotate-90" : ""} />
        {label}
      </button>
      <div id={detailsId} hidden={!open && !fullscreen}>
        {(open || fullscreen) && (
          <BoundedMessageContent collapsedHeight={"12rem"} as="pre"
            data-testid="message-raw-content"
            className="m-0 whitespace-pre-wrap break-words font-mono text-xs"
            style={{ color: "var(--ct-fg)" }}
          >
            {raw}
          </BoundedMessageContent>
        )}
        {(open || fullscreen) && details}
      </div>
      {children}
    </div>
    )}</ExpandableReader>
  );
}
