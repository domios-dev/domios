import type { ToolEvent } from "@codeterm/chat-core";
import type { ReactNode } from "react";

function durationLabel(durationMs: number): string {
  if (durationMs < 1000) return `${durationMs}ms`;
  const seconds = durationMs / 1000;
  return `${Number.isInteger(seconds) ? seconds : seconds.toFixed(1)}s`;
}

export function ToolOutcomeFacts({ event }: { event: ToolEvent }) {
  const facts: ReactNode[] = [];
  if (event.exitCode !== undefined) {
    facts.push(
      <span key="exit" data-testid="toolcard-exit-code" aria-label={`Exit code ${event.exitCode}`} title={`Exit code ${event.exitCode}`}>
        exit {event.exitCode}
      </span>,
    );
  }
  if (event.durationMs !== undefined) {
    facts.push(
      <span key="duration" data-testid="toolcard-duration" aria-label={`Duration ${event.durationMs} milliseconds`} title={`${event.durationMs}ms`}>
        {durationLabel(event.durationMs)}
      </span>,
    );
  }
  if (event.interrupted === true) {
    facts.push(<span key="interrupted" data-testid="toolcard-interrupted">interrupted</span>);
  }
  if (event.backgroundTaskId) {
    facts.push(
      <span key="background" data-testid="toolcard-background" title={event.backgroundTaskId}>
        background
      </span>,
    );
  }
  if (facts.length === 0) return null;
  return (
    <span data-testid="toolcard-outcome-facts" className="inline-flex shrink-0 items-center gap-1.5 font-mono text-[10.5px]" style={{ color: "var(--ct-muted-fg)" }}>
      {facts.map((fact, index) => (
        <span key={index} className="rounded px-1 py-0.5" style={{ background: "color-mix(in srgb, var(--ct-muted-fg) 10%, transparent)" }}>
          {fact}
        </span>
      ))}
    </span>
  );
}
