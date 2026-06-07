import type { ApiConfig } from "../../lib/types";
import { IS_WEB, apiHost, currentToken } from "../../lib/webApi";

export type GitRefKind = "head" | "branch" | "remote" | "tag";

export interface GitRef {
  name: string;
  kind: GitRefKind;
}

export interface GitCommit {
  sha: string;
  parents: string[];
  author: string;
  date: string;
  refs: GitRef[];
  subject: string;
}

export interface GitGraphResponse {
  commits: GitCommit[];
  head_sha: string | null;
  branch: string | null;
  dirty: boolean;
}

export type GitFileStatus = "added" | "modified" | "deleted" | "renamed" | "copied";

export interface GitCommitFile {
  path: string;
  old_path?: string | null;
  added: number;
  deleted: number;
  status: GitFileStatus;
}

export class NotARepoError extends Error {}

export function baseUrl(api: ApiConfig): string {
  if (IS_WEB) return apiHost();
  const host = api.host === "0.0.0.0" ? "127.0.0.1" : api.host;
  return `http://${host}:${api.port}`;
}

export function authHeaders(api: ApiConfig): HeadersInit {
  const token = IS_WEB ? currentToken() : api.token;
  return token ? { Authorization: `Bearer ${token}` } : {};
}

export async function fetchGraph(
  api: ApiConfig,
  cwd: string,
  limit: number,
  skip: number,
  signal?: AbortSignal,
): Promise<GitGraphResponse> {
  const qs = new URLSearchParams({ cwd, limit: String(limit), skip: String(skip) });
  const res = await fetch(`${baseUrl(api)}/api/git/graph?${qs}`, {
    headers: authHeaders(api),
    signal,
  });
  if (res.status === 404) throw new NotARepoError("not a git repository");
  if (!res.ok) throw new Error(`git graph failed: ${res.status} ${await res.text()}`);
  return res.json();
}

export async function fetchCommitFiles(
  api: ApiConfig,
  cwd: string,
  sha: string,
  signal?: AbortSignal,
): Promise<GitCommitFile[]> {
  const qs = new URLSearchParams({ cwd });
  const res = await fetch(
    `${baseUrl(api)}/api/git/commit/${encodeURIComponent(sha)}/files?${qs}`,
    { headers: authHeaders(api), signal },
  );
  if (res.status === 404) throw new NotARepoError("not a git repository");
  if (!res.ok) throw new Error(`git files failed: ${res.status} ${await res.text()}`);
  return res.json();
}

export interface RepoEntry {
  path: string;
  name: string;
  branch: string | null;
}

export async function fetchRepos(
  api: ApiConfig,
  cwd: string,
  signal?: AbortSignal,
): Promise<RepoEntry[]> {
  const qs = new URLSearchParams({ cwd });
  const res = await fetch(`${baseUrl(api)}/api/git/repos?${qs}`, {
    headers: authHeaders(api),
    signal,
  });
  if (!res.ok) return [];
  const body = (await res.json()) as { repos: RepoEntry[] };
  return body.repos;
}

export async function fetchCommitDiff(
  api: ApiConfig,
  cwd: string,
  sha: string,
  path: string,
  signal?: AbortSignal,
): Promise<string> {
  const qs = new URLSearchParams({ cwd, path });
  const res = await fetch(
    `${baseUrl(api)}/api/git/commit/${encodeURIComponent(sha)}/diff?${qs}`,
    { headers: authHeaders(api), signal },
  );
  if (res.status === 404) throw new NotARepoError("not a git repository");
  if (!res.ok) throw new Error(`git diff failed: ${res.status} ${await res.text()}`);
  return res.text();
}
