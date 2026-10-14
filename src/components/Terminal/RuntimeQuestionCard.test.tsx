import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import type { AgentRuntimeQuestion } from "../../lib/agentRuntimeQuestions";
import { RuntimeQuestionCard } from "./RuntimeQuestionCard";

const bridge = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("../../lib/bridge", () => bridge);
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

it("uses the existing question control and sends the native answer once through the durable command", async () => {
  const status = { send_id: "receipt", tab_id: "owned", state: "written" as const, error: null, ledger_id: "ledger", sequence: 1, retained_from: 1 };
  const item: AgentRuntimeQuestion = {
    threadId: "thread", itemId: "item", response: "message", createdAt: 1,
    questions: [{ id: "q", text: "Fixture question", options: [{ value: "native-a", label: "Display A" }], allowFreeText: false }],
  };
  bridge.invoke.mockResolvedValue(status);
  const receipt = vi.fn();
  const host = document.createElement("div");
  const root = createRoot(host);
  try {
    await act(async () => root.render(<RuntimeQuestionCard tabId="owned" item={item} onReceipt={receipt} />));
    const option = [...host.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent === "Display A")!;
    expect(option).not.toBeNull();
    expect(option.getAttribute("type")).toBe("button");
    expect(host.querySelector("input")).toBeNull();
    await act(async () => option.click());
    const submit = [...host.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent?.includes("Send answer"))!;
    await act(async () => { submit.click(); submit.click(); });
    expect(bridge.invoke).toHaveBeenCalledExactlyOnceWith("agent_runtime_answer_question", {
      tabId: "owned", itemId: "item", answers: [{ questionId: "q", values: ["native-a"] }],
    });
    expect(receipt).toHaveBeenCalledExactlyOnceWith(status);
    expect(host.querySelectorAll("button:not(:disabled)")).toHaveLength(0);
    await act(async () => root.render(<RuntimeQuestionCard tabId="owned" item={{ ...item, resolved: { answers: [], sendId: "receipt", status } }} onReceipt={receipt} />));
    expect(bridge.invoke).toHaveBeenCalledTimes(1);
  } finally { await act(async () => root.unmount()); }
});
