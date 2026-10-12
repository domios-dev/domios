export type ProviderHint = "google" | "facebook" | "email";
export function signinHint(query: URLSearchParams) {
  const provider = query.getAll("provider").length === 1 ? query.get("provider") : null;
  const known = provider === "google" || provider === "facebook" || provider === "email" ? provider : null;
  return { provider: known, mode: query.getAll("mode").length === 1 && query.get("mode") === "signup" ? "signup" as const : "signin" as const };
}
export function hintedAction(hint: ReturnType<typeof signinHint>, methods: readonly string[], emailLink: boolean) {
  if (emailLink) return "finish-email";
  if (hint.provider === "email" && !methods.includes("local")) return "email";
  return hint.provider && methods.includes(hint.provider) ? hint.provider : "choices";
}
