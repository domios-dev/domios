// Git plugin UI API — every call goes through the host bridge
// (window.ct.invoke) which forwards to the plugin's viewCall. The host stamps
// the pane cwd into args, so callers pass empty cwd and the host fills it.

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

export type GitFileStatus = "added" | "modified" | "deleted" | "renamed" | "copied" | "binary";

export interface GitCommitFile {
  path: string;
  old_path?: string | null;
  added: number | null;
  deleted: number | null;
  status: GitFileStatus;
}

export interface RepoEntry {
  path: string;
  name: string;
  branch: string | null;
}

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

export class NotARepoError extends Error {}

export const WORKDIR_SHA = "__workdir__";

declare global {
  interface Window {
    ct?: { invoke(method: string, args?: unknown): Promise<unknown>; close?(): void };
  }
}

// When the caller passes a concrete cwd (e.g. a selected repo) we forward it so
// multi-repo switching works; otherwise the host stamps the pane cwd in.
async function call<T>(
  method: string,
  cwd: string,
  args: Record<string, unknown> = {},
): Promise<T> {
  const ct = window.ct;
  if (!ct) throw new Error("bridge unavailable");
  const full = cwd ? { ...args, cwd } : args;
  const r = (await ct.invoke(method, full)) as T & { error?: string };
  if (r && typeof r === "object" && "error" in r && (r as { error?: string }).error) {
    throw new Error((r as { error: string }).error);
  }
  return r as T;
}

export async function fetchGraph(
  _api: unknown,
  cwd: string,
  limit: number,
  skip: number,
  _signal?: AbortSignal,
): Promise<GitGraphResponse> {
  return call<GitGraphResponse>("gitGraph", cwd, { limit, skip });
}

export async function fetchCommitFiles(
  _api: unknown,
  cwd: string,
  sha: string,
  _signal?: AbortSignal,
): Promise<GitCommitFile[]> {
  return call<GitCommitFile[]>("gitCommitFiles", cwd, { sha });
}

export async function fetchRepos(_api: unknown, cwd: string, _signal?: AbortSignal): Promise<RepoEntry[]> {
  try {
    return await call<RepoEntry[]>("gitRepos", cwd, {});
  } catch {
    return [];
  }
}

export async function fetchWorkdirFiles(
  _api: unknown,
  cwd: string,
  _signal?: AbortSignal,
): Promise<GitWorkdirFile[]> {
  return call<GitWorkdirFile[]>("gitWorkdirFiles", cwd, {});
}

export async function fetchWorkdirDiff(
  _api: unknown,
  cwd: string,
  path: string,
  _signal?: AbortSignal,
): Promise<string> {
  const r = await call<{ diff: string }>("gitWorkdirDiff", cwd, { path });
  return r.diff ?? "";
}

export async function revertFile(_api: unknown, cwd: string, path: string): Promise<void> {
  await call("gitRevert", cwd, { path });
}

export async function stageFile(_api: unknown, cwd: string, path: string): Promise<void> {
  await call("gitStage", cwd, { path });
}

export async function unstageFile(_api: unknown, cwd: string, path: string): Promise<void> {
  await call("gitUnstage", cwd, { path });
}

export async function stageFiles(_api: unknown, cwd: string, paths: string[]): Promise<void> {
  await call("gitStage", cwd, { paths });
}

export async function unstageFiles(_api: unknown, cwd: string, paths: string[]): Promise<void> {
  await call("gitUnstage", cwd, { paths });
}

export async function commitRepo(_api: unknown, cwd: string, message: string): Promise<void> {
  await call("gitCommit", cwd, { message });
}

export async function pushRepo(_api: unknown, cwd: string): Promise<void> {
  await call("gitPush", cwd, {});
}

export async function fetchCommitDiff(
  _api: unknown,
  cwd: string,
  sha: string,
  path: string,
  _signal?: AbortSignal,
): Promise<string> {
  const r = await call<{ diff: string }>("gitCommitDiff", cwd, { sha, path });
  return r.diff ?? "";
}
