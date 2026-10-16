export const CHAT_READABLE_WIDTH_REM = 56;

export function chatLayoutTokens(readableWidthRem = CHAT_READABLE_WIDTH_REM) {
  return {
    maxWidthRem: readableWidthRem,
    variables: { "--ct-chat-readable-width": `${readableWidthRem}rem` },
  };
}
