import { invoke } from "../../lib/bridge";

export const CHAT_REATTACHED_EVENT = "codeterm:chat-reattached";

export async function reattachPaneChat(paneId: string): Promise<void> {
  await invoke("chat_stop", { paneId });
  window.dispatchEvent(new CustomEvent(CHAT_REATTACHED_EVENT, { detail: { paneId } }));
}
