import React from "react";

interface Props {
  formatted: React.ReactNode;
  raw: string;
  formattedAvailable?: boolean;
  persistent?: React.ReactNode;
}

type MessageView = "formatted" | "raw";

export function MessageViewToggle({ formatted, raw, formattedAvailable = true, persistent }: Props) {
  const [requestedView, setRequestedView] = React.useState<MessageView>("formatted");
  const view = formattedAvailable ? requestedView : "raw";

  return (
    <div data-testid="message-view-toggle">
      <div
        role="group"
        aria-label="Message view"
        className="mb-2 inline-flex rounded-md border p-0.5 font-mono text-[10.5px]"
        style={{ borderColor: "var(--ct-border)", background: "var(--ct-bg)" }}
      >
        {(["formatted", "raw"] as const).map((candidate) => {
          const disabled = candidate === "formatted" && !formattedAvailable;
          const selected = view === candidate;
          return (
            <button
              key={candidate}
              type="button"
              aria-pressed={selected}
              disabled={disabled}
              onClick={() => setRequestedView(candidate)}
              className="rounded px-2 py-0.5 capitalize transition-colors disabled:cursor-not-allowed disabled:opacity-40"
              style={{
                color: selected ? "var(--ct-fg)" : "var(--ct-muted-fg)",
                background: selected ? "color-mix(in srgb, var(--ct-accent) 18%, transparent)" : "transparent",
              }}
            >
              {candidate === "formatted" ? "Formatted" : "Raw"}
            </button>
          );
        })}
      </div>
      {formattedAvailable && <div hidden={view !== "formatted"}>{formatted}</div>}
      {view === "raw" && (
        <pre
          data-testid="message-raw-content"
          className="max-h-48 overflow-auto whitespace-pre-wrap break-words font-mono text-xs"
          style={{ color: "var(--ct-fg)" }}
        >
          {raw}
        </pre>
      )}
      {persistent}
    </div>
  );
}
