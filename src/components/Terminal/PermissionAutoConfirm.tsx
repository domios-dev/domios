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
