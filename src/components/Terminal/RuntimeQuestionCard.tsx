import { useRef, useState } from "react";
import { InteractiveQuestion } from "@codeterm/chat-ui";
import { invoke } from "../../lib/bridge";
import { collectRuntimeQuestionAnswer, runtimeQuestionElement, runtimeQuestionProgress,
  type AgentRuntimeQuestion, type RuntimeQuestionAnswer } from "../../lib/agentRuntimeQuestions";
import type { ChatSendStatus } from "./chatSendStatus";
import { ChatDeliveryState } from "./ChatDeliveryState";

export function RuntimeQuestionCard({ tabId, item, onReceipt }: {
  tabId: string;
  item: AgentRuntimeQuestion;
  onReceipt: (status: ChatSendStatus) => void;
}) {
  const [draft, setDraft] = useState<RuntimeQuestionAnswer[]>([]);
  const [receipt, setReceipt] = useState<ChatSendStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const pending = useRef(false);
  const progress = runtimeQuestionProgress(item, draft);
  const status = item.resolved?.status ?? receipt;
  if (status) return <>
    {item.questions.map((question, index) => <InteractiveQuestion key={question.id} question={runtimeQuestionElement(item, index)} answered />)}
    <ChatDeliveryState state={status.state === "held" ? "queued" : status.state === "written" ? "sending"
      : status.state === "submit_unconfirmed" ? "uncertain" : status.state === "failed" ? "failed" : "delivered"} />
  </>;
  if (progress.index < 0) return null;
  const index = progress.index;
  return <>
    <InteractiveQuestion key={item.questions[index].id} question={runtimeQuestionElement(item, index)}
      onAnswer={async (_text, _question, selection) => {
        if (pending.current) return;
        const next = collectRuntimeQuestionAnswer(item, draft, item.questions[index].id, selection);
        const nextProgress = runtimeQuestionProgress(item, next);
        if (!nextProgress.complete) { setDraft(next); return; }
        pending.current = true;
        setError(null);
        try {
          const received = await invoke<ChatSendStatus>("agent_runtime_answer_question", { tabId, itemId: item.itemId, answers: nextProgress.answers });
          setReceipt(received);
          onReceipt(received);
        } catch (cause) {
          setError(cause instanceof Error ? cause.message : String(cause));
          throw cause;
        } finally { pending.current = false; }
      }} />
    {error && <div role="alert">{error}</div>}
  </>;
}
