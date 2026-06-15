import { useEffect, useState } from "react";
import {
  CHAT_UI_DEFAULT_PROMPT_EVENT,
  type ChatUiDefaultPromptChoice,
  type ChatUiDefaultPromptDetail,
} from "../lib/chatUiMode";

type Choose = (choice: ChatUiDefaultPromptChoice) => void;

export function ChatUiDefaultPrompt() {
  const [choose, setChoose] = useState<Choose | null>(null);

  useEffect(() => {
    const onPrompt = (event: Event) => {
      const detail = (event as CustomEvent<ChatUiDefaultPromptDetail>).detail;
      if (!detail || typeof detail.choose !== "function") return;
      detail.handled = true;
      setChoose(() => detail.choose);
    };

    window.addEventListener(CHAT_UI_DEFAULT_PROMPT_EVENT, onPrompt);
    return () => window.removeEventListener(CHAT_UI_DEFAULT_PROMPT_EVENT, onPrompt);
  }, []);

  if (!choose) return null;

  const resolve = (choice: ChatUiDefaultPromptChoice) => {
    const current = choose;
    setChoose(null);
    current(choice);
  };

  return (
    <div className="fixed inset-0 z-[100] flex items-center justify-center bg-black/45 px-4">
      <section
        role="dialog"
        aria-modal="true"
        aria-labelledby="chat-ui-default-title"
        className="w-full max-w-md rounded-lg border p-5 shadow-2xl"
        style={{
          background: "var(--ct-surface, #18181b)",
          borderColor: "var(--ct-border, rgba(255,255,255,0.16))",
          color: "var(--ct-fg, #f4f4f5)",
        }}
      >
        <h2 id="chat-ui-default-title" className="text-base font-semibold">
          Use Chat UI for agent panes?
        </h2>
        <p className="mt-2 text-sm leading-6" style={{ color: "var(--ct-muted-fg, #a1a1aa)" }}>
          New supported agent panes can open in Chat UI by default. You can change this later in Settings.
        </p>
        <div className="mt-5 flex justify-end gap-2">
          <button
            type="button"
            className="rounded-md border px-3 py-2 text-sm"
            style={{
              borderColor: "var(--ct-border, rgba(255,255,255,0.16))",
              background: "var(--ct-bg, #09090b)",
            }}
            onClick={() => resolve("no")}
          >
            Not now
          </button>
          <button
            type="button"
            className="rounded-md px-3 py-2 text-sm font-medium"
            style={{
              background: "var(--ct-accent, #3b82f6)",
              color: "white",
            }}
            onClick={() => resolve("yes")}
          >
            Use Chat UI
          </button>
        </div>
      </section>
    </div>
  );
}
