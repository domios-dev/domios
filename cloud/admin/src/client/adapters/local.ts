import type { SignInClient } from "../auth";
export function client(): SignInClient {
  return {
    async signIn(email, password) {
      const response = await fetch("/api/local-login", {
        method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ email, password }),
      });
      if (!response.ok) throw new Error("Sign-in failed");
      const result: { token: string } = await response.json();
      return result.token;
    },
  };
}
