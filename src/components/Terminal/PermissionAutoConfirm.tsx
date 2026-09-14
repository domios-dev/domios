import { ShieldCheck } from "lucide-react";
import type { AutoConfirmedPrompt } from "../../lib/types";

export function autoConfirmationsForSession(
  records: AutoConfirmedPrompt[] | undefined,
  sessionId: string | null,
): AutoConfirmedPrompt[] {
  if (!records || !sessionId) return [];
  return records.filter((record) => record.session_id === sessionId);
}

export function AutoConfirmedPromptRows({ records }: { records: AutoConfirmedPrompt[] }) {
  if (records.length === 0) return null;
  return (
    <ul
      className="mt-2 flex flex-col gap-1"
      aria-label="Auto-confirmed permission prompts"
      data-testid="auto-confirmed-prompts"
    >
      {records.map((record) => (
        <li
          key={record.prompt_fingerprint}
          data-testid="auto-confirmed-prompt"
          data-rule={record.rule}
          title={`${record.question}\n${record.options.join(" · ")}\n${record.rule}`}
          className="flex min-w-0 items-center gap-2 px-1 text-xs font-mono"
          style={{ color: "var(--ct-muted-fg)" }}
        >
          <ShieldCheck size={12} aria-hidden className="shrink-0" style={{ color: "var(--ct-green)" }} />
          <span className="truncate">
            auto-confirmed <span data-testid="auto-confirmed-tool">{record.tool_name}</span> via{" "}
            <span data-testid="auto-confirmed-option">{record.option_label}</span>
          </span>
        </li>
      ))}
    </ul>
  );
}

export function PermissionAutoConfirmToggle({
  enabled,
  onToggle,
}: {
  enabled: boolean;
  onToggle: (next: boolean) => void;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={enabled}
      aria-label="Auto-confirm recognized permission prompts"
      data-testid="permission-auto-confirm-toggle"
      onMouseDown={(e) => e.preventDefault()}
      onClick={() => onToggle(!enabled)}
      className="inline-flex items-center gap-1.5 rounded-md border px-2 py-0.5 text-[11px] font-medium transition-[filter] hover:brightness-125 focus-visible:outline focus-visible:outline-1"
      style={
        enabled
          ? {
              color: "var(--ct-green)",
              borderColor: "color-mix(in srgb, var(--ct-green) 45%, transparent)",
              background: "color-mix(in srgb, var(--ct-green) 12%, transparent)",
            }
          : {
              color: "var(--ct-muted-fg)",
              borderColor: "color-mix(in srgb, var(--ct-border) 60%, transparent)",
              background: "transparent",
            }
      }
    >
      <ShieldCheck size={12} aria-hidden />
      {enabled ? "Auto-confirm on" : "Auto-confirm off"}
    </button>
  );
}
