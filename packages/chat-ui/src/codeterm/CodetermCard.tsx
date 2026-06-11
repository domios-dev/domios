import type { CodetermInvocation } from "./recognize";
import { renderTemplate, type PaneNameResolver } from "./templates";

interface Props {
  inv: CodetermInvocation;
  result?: string;
  expanded: boolean;
  onToggle: () => void;
  resolvePaneName?: PaneNameResolver;
}

export function CodetermCard({ inv, result, expanded, onToggle, resolvePaneName }: Props) {
  const card = renderTemplate(inv, result, resolvePaneName);
  return (
    <div className="mb-1.5 w-full max-w-4xl min-w-0 mx-auto">
      <div className="inline-flex flex-col max-w-[min(52rem,100%)]">
        <button
          onClick={onToggle}
          className="inline-flex min-w-0 items-center gap-1.5 text-left rounded-md border px-2.5 py-1 text-xs transition-colors duration-150 hover:brightness-110"
          style={{ background: "transparent", borderColor: "var(--ct-border)", color: "var(--ct-fg)" }}
        >
          <span className="flex-shrink-0" style={{ color: "var(--ct-accent, #6ea8fe)" }}>
            ⬡
          </span>
          <span className="font-medium" style={{ color: "var(--ct-accent, #6ea8fe)" }}>
            CodeTerm
          </span>
          <span className="flex-shrink-0">{card.icon}</span>
          <span className="truncate">{card.title}</span>
          {card.status && (
            <span
              className="flex-shrink-0"
              style={{ color: card.status.ok ? "var(--ct-ok, #41d18a)" : "var(--ct-err, #ff6b6b)" }}
            >
              {card.status.ok ? "✓" : "✗"} {card.status.label}
            </span>
          )}
        </button>
        {card.detail && !expanded && (
          <div className="px-2.5 py-1 text-xs truncate" style={{ color: "var(--ct-muted-fg)" }}>
            {card.detail}
          </div>
        )}
        {expanded && (
          <div className="mt-1 rounded-md border px-2.5 py-1.5 text-xs" style={{ borderColor: "var(--ct-border)" }}>
            <pre className="whitespace-pre-wrap break-all" style={{ color: "var(--ct-muted-fg)" }}>
              {inv.raw}
            </pre>
            {result && (
              <pre className="mt-1 whitespace-pre-wrap break-all" style={{ color: "var(--ct-fg)" }}>
                {result}
              </pre>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
