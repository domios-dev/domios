import { useCallback, useState } from "react";
import { invoke } from "../lib/bridge";

export type AskState = "idle" | "sending" | "sent" | "error";

const RESET_MS = 4000;

/** Sends a prepared question to the General Agent tab (alias "main") and tracks the round trip. */
export function useAskGeneral() {
  const [state, setState] = useState<AskState>("idle");
  const ask = useCallback(async (question: string) => {
    setState("sending");
    try {
      await invoke("chat_send_text", { tabId: "main", text: question });
      setState("sent");
    } catch {
      setState("error");
    }
    setTimeout(() => setState("idle"), RESET_MS);
  }, []);
  return { state, ask };
}
