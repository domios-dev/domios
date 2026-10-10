import { useEffect, useState } from "react";
import { inputWriteFailureTarget, reduceInputWriteFailure, type InputWriteFailureState } from "./inputWriteFailureModel";
import { TERMINAL_INPUT_WRITE_FAILED_EVENT, TERMINAL_INPUT_WRITE_SUCCEEDED_EVENT } from "./terminalInputWrite";

export function useInputWriteFailure(tabId: string, ptyId: string | null) {
  const targetId = inputWriteFailureTarget(tabId, ptyId);
  const [failure, setFailure] = useState<InputWriteFailureState>(null);
  useEffect(() => {
    setFailure((state) => state === targetId ? state : null);
    const receive = (kind: "failure" | "success") => (event: Event) => {
      const detail: unknown = (event as CustomEvent<unknown>).detail;
      if (!detail || typeof detail !== "object" || !("targetId" in detail) || typeof detail.targetId !== "string") return;
      const eventTarget = detail.targetId;
      setFailure((state) => reduceInputWriteFailure(state, targetId, { kind, targetId: eventTarget }));
    };
    const failed = receive("failure");
    const succeeded = receive("success");
    window.addEventListener(TERMINAL_INPUT_WRITE_FAILED_EVENT, failed);
    window.addEventListener(TERMINAL_INPUT_WRITE_SUCCEEDED_EVENT, succeeded);
    return () => {
      window.removeEventListener(TERMINAL_INPUT_WRITE_FAILED_EVENT, failed);
      window.removeEventListener(TERMINAL_INPUT_WRITE_SUCCEEDED_EVENT, succeeded);
    };
  }, [targetId]);
  return {
    visible: targetId !== null && failure === targetId,
    dismiss: () => setFailure((state) => reduceInputWriteFailure(state, targetId, { kind: "dismiss" })),
  };
}
