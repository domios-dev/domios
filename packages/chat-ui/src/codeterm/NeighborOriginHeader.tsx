import { useId, useState } from "react";
import { Users } from "lucide-react";
import type { ChatOrigin } from "@codeterm/chat-core";
import { ChevronIcon } from "../CodeBlock";

type NeighborOrigin = Extract<ChatOrigin, { kind: "neighbor" }>;

/** "From <friend> via Neighbors" above the friend's own words; the host's ids and instruction fold away behind the chevron. */
export function NeighborOriginHeader({ origin }: { origin: NeighborOrigin }) {
  const [open, setOpen] = useState(false);
  const details = useId();
  const rows: Array<[string, string]> = [
    ["Friendship", origin.friendshipId],
    ...(origin.sender ? [["Sender", origin.sender] as [string, string]] : []),
    ...(origin.instruction ? [["Instruction", origin.instruction] as [string, string]] : []),
  ];
  return (
    <div className="mb-1 min-w-0" data-testid="chat-neighbor-origin">
      <button
        type="button"
        aria-expanded={open}
        aria-controls={details}
        onClick={() => setOpen(!open)}
        className="flex items-center gap-1.5 text-[11.5px] font-medium"
        style={{ color: "color-mix(in srgb, var(--ct-accent) 72%, var(--ct-muted-fg))", background: "transparent", border: "none", padding: 0, cursor: "pointer" }}
      >
        <Users size={12} aria-hidden="true" />
        <span>From {origin.friend ?? "a friend"} via Neighbors</span>
        <ChevronIcon expanded={open} className="opacity-60" />
      </button>
      {open && (
        <dl id={details} className="mt-1 grid min-w-0 grid-cols-[auto_minmax(0,1fr)] gap-x-2 gap-y-0.5 text-[11px]" style={{ color: "var(--ct-muted-fg)" }}>
          {rows.map(([label, value]) => (
            <div key={label} className="contents">
              <dt>{label}</dt>
              <dd className="m-0 min-w-0 select-text [overflow-wrap:anywhere]">{value}</dd>
            </div>
          ))}
        </dl>
      )}
    </div>
  );
}
