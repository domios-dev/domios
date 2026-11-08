export interface AuthMethod {id: string; name: string; type: "agent" | "terminal" | "unsupported"}
export type AuthState = "authenticated" | "needs_auth" | "unavailable" | "unknown";
export function decodeAuthMethods(value: unknown): AuthMethod[] {
  if (value === undefined) return [];
  if (!Array.isArray(value) || value.length > 32) throw new Error("Invalid ACP authMethods");
  const ids = new Set<string>();
  return value.map(raw => {
    if (!raw || typeof raw !== "object" || Array.isArray(raw)) throw new Error("Invalid ACP auth method");
    const method = raw as Record<string, unknown>;
    if (typeof method.id !== "string" || !method.id.length || method.id.length > 128 || /[\u0000-\u001f]/.test(method.id)
      || typeof method.name !== "string" || !method.name.length || method.name.length > 256 || ids.has(method.id)) throw new Error("Invalid ACP auth method");
    ids.add(method.id);
    return {id: method.id, name: method.name, type: method.type === undefined || method.type === "agent" ? "agent" : method.type === "terminal" ? "terminal" : "unsupported"};
  });
}
export function authenticationState(methods: readonly AuthMethod[] | undefined, evidence: {authenticated?: boolean; required?: boolean; keyPresent?: boolean} = {}): AuthState {
  if (!methods) return "unknown";
  if (evidence.authenticated) return "authenticated";
  if (evidence.required || evidence.keyPresent === false) return "needs_auth";
  if (methods.length && methods.every(method => method.type !== "agent")) return "unavailable";
  return "unknown";
}
export function agentAuthMethod(methods: readonly AuthMethod[], id: unknown): AuthMethod {
  const method = methods.find(method => method.id === id && method.type === "agent");
  if (!method) throw new Error("Authentication method was not declared or is unavailable");
  return method;
}
