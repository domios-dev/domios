import { useId, type ReactNode } from "react";

export interface BoardPanelProps {
  testId: string;
  title: string;
  meta?: ReactNode;
  className?: string;
  children: ReactNode;
}

/** A contained Board section: a ruled header naming it, then its readings. */
export function BoardPanel({ testId, title, meta, className, children }: BoardPanelProps) {
  const headingId = useId();
  return (
    <section
      data-testid={testId}
      aria-labelledby={headingId}
      className={`ct-panel-card mc-board-panel${className ? ` ${className}` : ""}`}
    >
      <header className="mc-board-panel-head">
        <h2 id={headingId}>{title}</h2>
        {meta != null && (
          <span data-testid={`${testId}-meta`} className="mc-board-panel-meta">
            {meta}
          </span>
        )}
      </header>
      {children}
    </section>
  );
}

export function BoardPanelSection({
  testId,
  label,
  children,
}: {
  testId?: string;
  label?: ReactNode;
  children: ReactNode;
}) {
  return (
    <div data-testid={testId} className="mc-board-sub">
      {label != null && <h3 className="mc-board-sub-label">{label}</h3>}
      {children}
    </div>
  );
}

/** Short readings that wrap between each other, never inside one. */
export function Facts({ items }: { items: Array<{ key: string; node: ReactNode }> }) {
  return (
    <ul className="mc-board-facts" role="list">
      {items.map((item) => (
        <li key={item.key} className="mc-board-fact">
          {item.node}
        </li>
      ))}
    </ul>
  );
}
