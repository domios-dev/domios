import type { NormalizedChatMessage } from "@codeterm/chat-core";

export const phoneAsk: NormalizedChatMessage = {
  id: "mob-phone-ask", type: "user", content: "How’s the cut looking?",
};
export const phoneReply: NormalizedChatMessage = {
  id: "mob-phone-reply", type: "assistant", content: "The new cut is ready. Want me to keep the quieter ending?",
};
export const phoneAnswer: NormalizedChatMessage = {
  id: "mob-phone-answer", type: "user", content: "Yes, keep that ending.",
};
export const phoneConfirm: NormalizedChatMessage = {
  id: "mob-phone-confirm", type: "assistant", content: "On it.",
};
