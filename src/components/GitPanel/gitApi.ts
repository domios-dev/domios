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
  branch_base?: string | null;
  branch_added?: number | null;
  branch_deleted?: number | null;
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

export const WORKDIR_SHA = "__workdir__";

export type GitWorkdirStatus =
  | "staged"
  | "modified"
  | "untracked"
  | "deleted"
  | "renamed"
  | "conflicted";

export interface GitWorkdirFile {
  path: string;
  old_path?: string | null;
  status: GitWorkdirStatus;
  staged: boolean;
  added: number | null;
  deleted: number | null;
}

export async function fetchWorkdirFiles(
  api: ApiConfig,
  cwd: string,
  signal?: AbortSignal,
): Promise<GitWorkdirFile[]> {
  const qs = new URLSearchParams({ cwd });
  const res = await fetch(`${baseUrl(api)}/api/git/workdir-files?${qs}`, {
    headers: authHeaders(api),
    signal,
  });
  if (res.status === 404) throw new NotARepoError("not a git repository");
  if (!res.ok) throw new Error(`git workdir files failed: ${res.status} ${await res.text()}`);
  return res.json();
}

export async function fetchWorkdirDiff(
  api: ApiConfig,
  cwd: string,
  path: string,
  signal?: AbortSignal,
): Promise<string> {
  const qs = new URLSearchParams({ cwd, path });
  const res = await fetch(`${baseUrl(api)}/api/git/workdir-diff?${qs}`, {
    headers: authHeaders(api),
    signal,
  });
  if (res.status === 404) throw new NotARepoError("not a git repository");
  if (!res.ok) throw new Error(`git workdir diff failed: ${res.status} ${await res.text()}`);
  return res.text();
}

export async function revertFile(api: ApiConfig, cwd: string, path: string): Promise<void> {
  const res = await fetch(`${baseUrl(api)}/api/git/revert-file`, {
    method: "POST",
    headers: { ...authHeaders(api), "Content-Type": "application/json" },
    body: JSON.stringify({ cwd, path }),
  });
  if (!res.ok) throw new Error((await res.text()) || `revert failed: ${res.status}`);
}

export async function stageFile(api: ApiConfig, cwd: string, path: string): Promise<void> {
  const res = await fetch(`${baseUrl(api)}/api/git/stage-file`, {
    method: "POST",
    headers: { ...authHeaders(api), "Content-Type": "application/json" },
    body: JSON.stringify({ cwd, path }),
  });
  if (!res.ok) throw new Error((await res.text()) || `stage failed: ${res.status}`);
}

export async function unstageFile(api: ApiConfig, cwd: string, path: string): Promise<void> {
  const res = await fetch(`${baseUrl(api)}/api/git/unstage-file`, {
    method: "POST",
    headers: { ...authHeaders(api), "Content-Type": "application/json" },
    body: JSON.stringify({ cwd, path }),
  });
  if (!res.ok) throw new Error((await res.text()) || `unstage failed: ${res.status}`);
}

export async function stageFiles(api: ApiConfig, cwd: string, paths: string[]): Promise<void> {
  const res = await fetch(`${baseUrl(api)}/api/git/stage-files`, {
    method: "POST",
    headers: { ...authHeaders(api), "Content-Type": "application/json" },
    body: JSON.stringify({ cwd, paths }),
  });
  if (!res.ok) throw new Error((await res.text()) || `stage failed: ${res.status}`);
}

export async function unstageFiles(api: ApiConfig, cwd: string, paths: string[]): Promise<void> {
  const res = await fetch(`${baseUrl(api)}/api/git/unstage-files`, {
    method: "POST",
    headers: { ...authHeaders(api), "Content-Type": "application/json" },
    body: JSON.stringify({ cwd, paths }),
  });
  if (!res.ok) throw new Error((await res.text()) || `unstage failed: ${res.status}`);
}

export async function commitRepo(api: ApiConfig, cwd: string, message: string): Promise<void> {
  const res = await fetch(`${baseUrl(api)}/api/git/commit`, {
    method: "POST",
    headers: { ...authHeaders(api), "Content-Type": "application/json" },
    body: JSON.stringify({ cwd, message }),
  });
  if (!res.ok) throw new Error((await res.text()) || `commit failed: ${res.status}`);
}

export async function pushRepo(api: ApiConfig, cwd: string): Promise<void> {
  const res = await fetch(`${baseUrl(api)}/api/git/push`, {
    method: "POST",
    headers: { ...authHeaders(api), "Content-Type": "application/json" },
    body: JSON.stringify({ cwd }),
  });
  if (!res.ok) throw new Error((await res.text()) || `push failed: ${res.status}`);
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
