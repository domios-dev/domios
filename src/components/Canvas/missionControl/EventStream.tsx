import type { StreamEvent } from "./types";

interface EventStreamProps {
  events: StreamEvent[];
}

export function EventStream({ events }: EventStreamProps) {
  return (
    <div className="mc-event-layer" aria-hidden data-testid="mission-event-stream">
      {events.map((ev, i) => (
        <span
          key={ev.id}
          className="mc-event-dot"
          style={{
            left: `${ev.fromX}px`,
            top: `${ev.fromY}px`,
            animationDelay: `${i * 180}ms`,
            // Fly toward top HUD band
            ["--mc-from-x" as string]: "0px",
            ["--mc-from-y" as string]: "0px",
            ["--mc-to-x" as string]: `${120 - ev.fromX}px`,
            ["--mc-to-y" as string]: `${-ev.fromY + 24}px`,
          }}
          title={ev.label}
        />
      ))}
    </div>
  );
}
