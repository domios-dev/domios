import type { ReactNode } from "react";
import type { PendingPrompt } from "../../lib/types";
import { PendingPromptCard } from "./PendingPromptCard";

export function ChatTranscriptViewport({ children, prompt, onRespond, bottomInset }: {
  children: ReactNode;
  prompt: { id: string; prompt: PendingPrompt } | null;
  onRespond: (value: string) => void;
  bottomInset: number;
}) {
  return <div className="relative flex-1 min-h-0">
    {children}
    {prompt && <section
      aria-label="Agent question"
      aria-live="polite"
      tabIndex={0}
      className="absolute inset-x-5 z-30 max-h-[50%] overflow-y-auto overscroll-contain rounded-lg border p-3 shadow-lg"
      style={{ bottom: bottomInset + 12, background: "var(--ct-bg)", borderColor: "var(--ct-border)" }}
    >
      <PendingPromptCard key={prompt.id} pendingPrompt={prompt.prompt} onRespond={onRespond} />
    </section>}
  </div>;
}
