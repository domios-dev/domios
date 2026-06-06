export type {
  ChatAttachment,
  ChatMessageKind,
  NormalizedChatMessage,
  ProviderChatAdapter,
  ChatOrigin,
  SlashCommand,
} from "./types";
export {
  chatMessageFingerprint,
  normalizeChatText,
  type FingerprintableChatMessage,
} from "./normalize";
export {
  dedupeRecentUserMessages,
  matchesRecentUserMessage,
} from "./dedupe";
