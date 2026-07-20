export const CHAT_REATTACHED_EVENT = "codeterm:chat-reattached";

export async function reattachPaneChat(paneId: string): Promise<void> {
  window.dispatchEvent(new CustomEvent(CHAT_REATTACHED_EVENT, { detail: { paneId } }));
}
