import { useId, useState } from "react";
import { Users } from "lucide-react";
import type { ChatOrigin } from "@codeterm/chat-core";
import { ChevronIcon } from "../CodeBlock";

type NeighborOrigin = Extract<ChatOrigin, { kind: "neighbor" }>;

const KIND_LABEL: Record<NonNullable<NeighborOrigin["messageKind"]>, string> = { question: "Question", answer: "Answer", refusal: "Refusal" };

/** "From <friend> via Neighbors" with what kind of message it is and what it answers, above the friend's own words;
 *  the host's ids and reply instruction fold away behind the chevron. */
export function NeighborOriginHeader({ origin }: { origin: NeighborOrigin }) {
  const [open, setOpen] = useState(false);
  const details = useId();
  const rows: Array<[string, string]> = [
    ["Friendship", origin.friendshipId],
    ...(origin.message ? [["Message", origin.message] as [string, string]] : []),
    ...(origin.inReplyTo ? [["In reply to", origin.inReplyTo] as [string, string]] : []),
    ...(origin.sender ? [["Sender", origin.sender] as [string, string]] : []),
    ...(origin.instruction ? [["Instruction", origin.instruction] as [string, string]] : []),
  ];
  // The answered message is earlier in this chat when it came through here too; otherwise its id stays in the details.
  const openReplied = () => document.querySelector(`[data-neighbor-message="${CSS.escape(origin.inReplyTo ?? "")}"]`)
    ?.scrollIntoView({ block: "center", behavior: "smooth" });
  return (
    <div className="mb-1 min-w-0" data-testid="chat-neighbor-origin" data-neighbor-message={origin.message}>
      <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-0.5 text-[11.5px]">
        <button
          type="button"
          aria-expanded={open}
          aria-controls={details}
          onClick={() => setOpen(!open)}
          className="flex items-center gap-1.5 font-medium"
          style={{ color: "color-mix(in srgb, var(--ct-accent) 72%, var(--ct-muted-fg))", background: "transparent", border: "none", padding: 0, cursor: "pointer" }}
        >
          <Users size={12} aria-hidden="true" />
          <span>From {origin.friend ?? "a friend"} via Neighbors</span>
          <ChevronIcon expanded={open} className="opacity-60" />
        </button>
        {origin.messageKind && (
          <span
            data-testid="chat-neighbor-kind"
            className="rounded-full border px-1.5 text-[10.5px] leading-4"
            style={{
              borderColor: origin.messageKind === "refusal" ? "color-mix(in srgb, var(--ct-red) 35%, transparent)" : "var(--ct-border)",
              color: origin.messageKind === "refusal" ? "color-mix(in srgb, var(--ct-red) 70%, var(--ct-muted-fg))" : "var(--ct-muted-fg)",
            }}
          >
            {KIND_LABEL[origin.messageKind]}
          </span>
        )}
        {origin.inReplyTo && (
          <button
            type="button"
            onClick={openReplied}
            className="hover:underline"
            style={{ color: "var(--ct-muted-fg)", background: "transparent", border: "none", padding: 0, cursor: "pointer", textUnderlineOffset: 2 }}
          >
            in reply to an earlier message
          </button>
        )}
      </div>
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
