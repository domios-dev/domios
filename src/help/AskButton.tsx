import { useState } from "react";
import { Bot, Check, Loader2 } from "lucide-react";
import { invoke } from "../lib/bridge";
import { BusySpinner } from "../components/shared/BusyState";

type AskState = "idle" | "sending" | "sent" | "error";

/** Sends the question to the General Agent pane (alias "main") and reports state inline. */
export function AskButton({ label, question }: { label: string; question: string }) {
  const [state, setState] = useState<AskState>("idle");

  const onClick = async () => {
    if (state === "sending") return;
    setState("sending");
    try {
      await invoke("chat_send_text", { paneId: "main", text: question });
      setState("sent");
      setTimeout(() => setState("idle"), 4000);
    } catch {
      setState("error");
      setTimeout(() => setState("idle"), 4000);
    }
  };

  return (
    <button
      type="button"
      data-testid="help-ask"
      data-ask-state={state}
      onClick={onClick}
      disabled={state === "sending"}
      className={`group flex w-full items-center gap-2.5 rounded-lg border px-3 py-2.5 text-left text-sm transition-colors ${
        state === "sent"
          ? "border-[var(--ct-green,#7ee787)]/40 bg-[color-mix(in_srgb,var(--ct-green,#7ee787)_8%,transparent)]"
          : "border-[var(--ct-accent)]/30 bg-[color-mix(in_srgb,var(--ct-accent)_6%,transparent)] hover:bg-[color-mix(in_srgb,var(--ct-accent)_12%,transparent)]"
      }`}
    >
      <span className="flex h-7 w-7 shrink-0 items-center justify-center rounded-md bg-[color-mix(in_srgb,var(--ct-accent)_14%,transparent)] text-[var(--ct-accent)]">
        {state === "sending" ? (
          <BusySpinner icon={Loader2} size={14} />
        ) : state === "sent" ? (
          <Check size={14} />
        ) : (
          <Bot size={14} />
        )}
      </span>
      <span className="min-w-0 flex-1 text-[var(--ct-fg)]">
        {state === "sent"
          ? "Sent — open the General tab for the answer"
          : state === "error"
            ? "Could not reach the General Agent — is it running?"
            : label}
      </span>
    </button>
  );
}
