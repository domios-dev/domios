"use client";
import Link from "next/link";
import { useEffect, useState } from "react";
import type { UserSummary } from "../../src/domain/model";
import { matchesPageFilters, nextPageStack, previousPageStack } from "../../src/domain/users";

export default function UserList() {
  const [pageSize, setPageSize] = useState(25);
  const [tokens, setTokens] = useState<string[]>([]);
  const [revision, setRevision] = useState(0);
  const [page, setPage] = useState<{ users: UserSummary[]; nextPageToken?: string }>();
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [filters, setFilters] = useState({ provider: "", disabled: "", profile: "" });
  useEffect(() => {
    const controller = new AbortController();
    setLoading(true); setError(""); setPage(undefined);
    const params = new URLSearchParams({ pageSize: String(pageSize) });
    if (tokens.length) params.set("pageToken", tokens.at(-1)!);
    void fetch(`/api/users?${params}`, { signal: controller.signal, cache: "no-store" }).then(async response => {
      const result = await response.json();
      if (!response.ok) throw new Error(result.error ?? "Could not load users");
      if (!controller.signal.aborted) setPage(result);
    }).catch(error => {
      if (!controller.signal.aborted) setError(error instanceof Error ? error.message : "Could not load users");
    }).finally(() => { if (!controller.signal.aborted) setLoading(false); });
    return () => controller.abort();
  }, [pageSize, tokens, revision]);
  const visible = page?.users.filter(user => matchesPageFilters(user, filters)) ?? [];
  return <section aria-label="Users list" aria-busy={loading}>
    <form onSubmit={event => event.preventDefault()}>
      <label>Page size<select value={pageSize} onChange={event => { setPageSize(Number(event.target.value)); setTokens([]); }}>
        {[25, 50, 100].map(size => <option key={size}>{size}</option>)}
      </select></label>
      <label>Provider<select value={filters.provider} onChange={event => setFilters({ ...filters, provider: event.target.value })}>
        <option value="">All</option>{["google", "facebook", "email", "anonymous"].map(provider => <option key={provider}>{provider}</option>)}
      </select></label>
      <label>Disabled<select value={filters.disabled} onChange={event => setFilters({ ...filters, disabled: event.target.value })}>
        <option value="">All</option><option value="yes">Yes</option><option value="no">No</option>
      </select></label>
      <label>Has profile<select value={filters.profile} onChange={event => setFilters({ ...filters, profile: event.target.value })}>
        <option value="">All</option><option value="yes">Yes</option><option value="no">No</option>
      </select></label>
      <button type="button" disabled={loading} onClick={() => setRevision(value => value + 1)}>Refresh</button>
    </form>
    <p>Filters apply only to the loaded page.</p>
    {loading && <p role="status">Loading users…</p>}
    {error && <p role="alert">{error}</p>}
    {page && <>
      <p role="status">Page {tokens.length + 1} · {page.users.length ? `Accounts ${tokens.length * pageSize + 1}–${tokens.length * pageSize + page.users.length}` : "No accounts"} · {visible.length} shown</p>
      {visible.length ? <table><thead><tr><th scope="col">User</th><th scope="col">Providers</th><th scope="col">Disabled</th><th scope="col">Has profile</th><th scope="col">Created</th><th scope="col">Last sign-in</th></tr></thead>
        <tbody>{visible.map(user => <tr key={user.uid}>
          <td><Link href={`/users/${encodeURIComponent(user.uid)}`}>{user.email ?? user.uid}</Link><br />{user.uid}</td>
          <td>{user.providers.join(", ") || "None"}</td><td>{user.disabled ? "Yes" : "No"}</td><td>{user.hasProfile ? "Yes" : "No"}</td>
          <td>{user.createdAt}</td><td>{user.lastSignInAt ?? "Never"}</td>
        </tr>)}</tbody></table> : <p role="status">No users match this page.</p>}
    </>}
    <form onSubmit={event => event.preventDefault()}>
      <button type="button" disabled={loading || !tokens.length} onClick={() => setTokens(previousPageStack)}>Previous</button>
      <button type="button" disabled={loading || !page?.nextPageToken} onClick={() => setTokens(nextPageStack(tokens, page?.nextPageToken))}>Next</button>
    </form>
  </section>;
}
