"use client";
import { useState, type FormEvent } from "react";
import { signInClient } from "../../src/client/auth";
export default function SignIn({ provider }: { provider: string }) {
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setError(""); setBusy(true);
    const form = new FormData(event.currentTarget);
    try {
      const client = await signInClient(provider);
      const token = await client.signIn(String(form.get("email")), String(form.get("password")));
      const response = await fetch("/api/session", {
        method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ token }),
      });
      if (!response.ok) throw new Error("Admin access required. Sign in again after your admin claim is granted.");
      window.location.assign("/users");
    } catch (error) { setError(error instanceof Error ? error.message : "Sign-in failed"); setBusy(false); }
  }
  return <><h1>Domios Admin</h1><p>Sign in to manage users and entitlements.</p>
    {provider === "local" && <p>Local development: admin@localhost / local-admin. Test user: user@localhost.</p>}
    <form onSubmit={submit}>
      <label>Email<input name="email" type="email" autoComplete="username" required /></label>
      <label>Password<input name="password" type="password" autoComplete="current-password" required /></label>
      <button disabled={busy}>{busy ? "Signing in…" : "Sign in"}</button>
    </form><p role="alert">{error}</p></>;
}
