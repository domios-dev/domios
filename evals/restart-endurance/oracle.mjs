export function textContent(content) {
  if (typeof content === "string") return content;
  if (Array.isArray(content)) return content.map(textContent).join("\n");
  return content && typeof content === "object" ? textContent(content.text ?? content.content ?? "") : "";
}
export const kind = (message) => message.type ?? message.role;

export function exactReply(messages, baseline, prompt, marker) {
  const fresh = messages.slice(baseline);
  const boundary = fresh.findIndex((message) => kind(message) === "user" && textContent(message.content).trim() === prompt);
  if (boundary < 0) return null;
  const following = fresh.slice(boundary + 1);
  const nextUser = following.findIndex((message) => kind(message) === "user");
  const turn = nextUser < 0 ? following : following.slice(0, nextUser);
  const answer = turn.filter((message) => kind(message) === "assistant")
    .map((message) => textContent(message.content).trim()).filter(Boolean).join("\n");
  return answer === marker ? answer : null;
}
