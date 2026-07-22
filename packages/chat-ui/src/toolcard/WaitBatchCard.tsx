import type { ToolEvent } from "@codeterm/chat-core";
import { useId, useState, type KeyboardEvent } from "react";

function elapsedLabel(elapsedMs: number | undefined): string | undefined {
  if (elapsedMs === undefined) return undefined;
  return elapsedMs < 1000 ? `${elapsedMs}ms` : `${(elapsedMs / 1000).toFixed(elapsedMs % 1000 === 0 ? 0 : 1)}s`;
}

function entryLabel(event: ToolEvent): string {
  return event.summary || event.message.toolName?.trim() || "wait";
}

export function WaitBatchCard({ event }: { event: ToolEvent }) {
  const [expanded, setExpanded] = useState(false);
  const detailsId = useId();
  const batch = event.waitBatch;
  if (!batch) return null;
  const elapsed = elapsedLabel(batch.elapsedMs);
  const count = batch.members.length;
  const label = `${expanded ? "Collapse" : "Expand"} ${count} wait ${count === 1 ? "event" : "events"}`;
  const toggleFromKey = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (event.key !== "Enter" && event.key !== " ") return;
    event.preventDefault();
    setExpanded((value) => !value);
  };

  return (
    <section data-testid="chat-wait-batch" className="mx-auto mb-1.5 w-full max-w-4xl rounded-lg border px-3 py-2" style={{ borderColor: "var(--ct-border)", background: "var(--ct-surface)" }}>
      <button
        type="button"
        aria-label={label}
        aria-expanded={expanded}
        aria-controls={detailsId}
        onClick={() => setExpanded((value) => !value)}
        onKeyDown={toggleFromKey}
        className="flex w-full items-center gap-2 text-left text-xs"
      >
        <span className="font-semibold" style={{ color: "var(--ct-accent)" }}>{count} wait {count === 1 ? "event" : "events"}</span>
        {elapsed && <span style={{ color: "var(--ct-muted-fg)" }}>{elapsed}</span>}
      </button>
      <div id={detailsId} role="region" aria-label="Wait event details" hidden={!expanded} className="mt-2 space-y-2 border-t pt-2 text-xs" style={{ borderColor: "var(--ct-border)" }}>
          {batch.members.map((member, index) => (
            <div key={`${event.id}:${index}`} className="rounded border px-2 py-1.5" style={{ borderColor: "var(--ct-border)" }}>
              <div>{entryLabel(member)} <span style={{ color: "var(--ct-muted-fg)" }}>· {member.status}</span></div>
              {member.message.timestamp && <div style={{ color: "var(--ct-muted-fg)" }}>{member.message.timestamp}</div>}
              {member.detail && <pre className="m-0 mt-1 whitespace-pre-wrap">{member.detail}</pre>}
              {member.result && <pre className="m-0 mt-1 whitespace-pre-wrap">{member.result}</pre>}
            </div>
          ))}
      </div>
    </section>
  );
}
