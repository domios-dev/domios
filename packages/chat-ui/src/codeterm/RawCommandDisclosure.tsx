import React from "react";
import { ChevronDown } from "lucide-react";

interface Props {
  raw: string;
  children?: React.ReactNode;
  /** Shown with the raw command, and hidden with it. */
  details?: React.ReactNode;
}

export function RawCommandDisclosure({ raw, children, details }: Props) {
  const [open, setOpen] = React.useState(false);
  const detailsId = React.useId();

  return (
    <div data-testid="raw-command-disclosure" className="grid gap-0.5">
      <button
        type="button"
        aria-expanded={open}
        aria-controls={detailsId}
        onClick={() => setOpen((value) => !value)}
        className="inline-flex w-fit items-center gap-1 py-0 font-mono text-[10.5px] transition-colors hover:brightness-125"
        style={{ color: "var(--ct-muted-fg)", background: "transparent" }}
      >
        <ChevronDown aria-hidden="true" size={12} strokeWidth={1.8} className={open ? "rotate-180" : ""} />
        {open ? "Hide raw command" : "Show raw command"}
      </button>
      {open && (
        <pre
          id={detailsId}
          data-testid="message-raw-content"
          className="m-0 max-h-48 overflow-auto whitespace-pre-wrap break-words font-mono text-xs"
          style={{ color: "var(--ct-fg)" }}
        >
          {raw}
        </pre>
      )}
      {open && details}
      {children}
    </div>
  );
}
