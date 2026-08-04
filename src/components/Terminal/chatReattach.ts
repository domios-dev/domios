export const CHAT_REATTACHED_EVENT = "codeterm:chat-reattached";

export async function reattachTabChat(tabId: string): Promise<void> {
  window.dispatchEvent(new CustomEvent(CHAT_REATTACHED_EVENT, { detail: { tabId } }));
}
