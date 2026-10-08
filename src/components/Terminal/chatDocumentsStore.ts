import { useSyncExternalStore } from "react";
import { invoke } from "../../lib/bridge";
import { createChatDocumentsModel, type DocumentFileCheck } from "./chatDocumentsModel";

export const chatDocumentsStore = createChatDocumentsModel((paths, tabId) => invoke<DocumentFileCheck>("check_local_preview_files", { paths, tabId }));
export function useChatDocumentSnapshot(tabId: string) {
  return useSyncExternalStore(chatDocumentsStore.subscribe, () => chatDocumentsStore.get(tabId));
}
export function useChatDocuments(tabId: string) {
  return useChatDocumentSnapshot(tabId).documents;
}
