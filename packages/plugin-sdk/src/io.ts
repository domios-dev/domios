import type { DirEntry, ExecOpts, ExecPollResult, FetchOpts, FetchResult } from "./host";

export type HostIoRequest =
  | { op: "fileExists" | "isSymlink" | "readFile" | "canonicalPath" | "lockFile" | "removeFile" | "makeDirs" | "readDir" | "listChildDirs"; path: string }
  | { op: "readFileHead" | "readFileTail"; path: string; maxBytes: number }
  | { op: "writeFile" | "writeFileAtomic"; path: string; contents: string }
  | { op: "unlockFile"; id: string }
  | { op: "writeTempFile"; contents: string; suffix?: string | null }
  | { op: "projectWriteAllowed"; root: string }
  | { op: "walkDir"; path: string; maxDepth: number }
  | { op: "sqliteQuery"; db: string; sql: string; params?: unknown[] | null; nullable?: boolean }
  | { op: "jsonBooleanRecord"; path: string; keys: string[]; grant: boolean }
  | { op: "providerVersion" | "receiverTarget" | "receiverTargetClear" }
  | { op: "credentialPublic"; id: string }
  | { op: "secretGet" | "secretDelete"; name: string }
  | { op: "secretSet"; name: string; value: string }
  | { op: "memSearch" | "workspaceEnsure" | "workerStart"; options: unknown }
  | { op: "execStart"; options: ExecOpts }
  | { op: "fetchStart"; options: FetchOpts }
  | { op: "analyticsQuery"; query: unknown }
  | { op: "workspaceTabs"; workspace: string }
  | { op: "workerPoll"; job: string }
  | { op: "agentSpawn"; workspace: string; options: unknown }
  | { op: "agentSend"; session: string; text: string }
  | { op: "agentPoll" | "agentReap"; session: string }
  | { op: "awaitJob"; job: string };

export type HostIoSteps<T> = Generator<HostIoRequest, T, unknown>;

export interface HostIoFs {
  readFile(path: string): HostIoSteps<string | null>;
  readFileHead(path: string, maxBytes: number): HostIoSteps<string | null>;
  readFileTail(path: string, maxBytes: number): HostIoSteps<string | null>;
  writeFile(path: string, contents: string): HostIoSteps<boolean>;
  writeFileAtomic(path: string, contents: string): HostIoSteps<boolean>;
  lockFile(path: string): HostIoSteps<string | null>;
  unlockFile(id: string): HostIoSteps<boolean>;
  removeFile(path: string): HostIoSteps<boolean>;
  makeDirs(path: string): HostIoSteps<boolean>;
  fileExists(path: string): HostIoSteps<boolean>;
  isSymlink(path: string): HostIoSteps<boolean | null>;
  canonicalPath(path: string): HostIoSteps<string | null>;
  writeTempFile(contents: string, suffix?: string): HostIoSteps<string | null>;
  readDir(path: string): HostIoSteps<DirEntry[]>;
  listChildDirs(path: string): HostIoSteps<string[] | null>;
  walkDir(path: string, maxDepth?: number): HostIoSteps<DirEntry[]>;
  sqliteQuery<T = Record<string, unknown>>(db: string, sql: string, params?: unknown[] | null): HostIoSteps<T[]>;
  sqliteQueryNullable<T = Record<string, unknown>>(db: string, sql: string, params?: unknown[] | null): HostIoSteps<T[] | null>;
  readJson<T = unknown>(path: string): HostIoSteps<T | null>;
  writeJson(path: string, value: unknown): HostIoSteps<boolean>;
}

export interface HostIo {
  <T>(request: HostIoRequest, then: (value: unknown) => T): T;
  run<T>(steps: HostIoSteps<T>): T;
  fs: HostIoFs;
  exec(options: ExecOpts): HostIoSteps<ExecPollResult>;
  fetch(options: FetchOpts): HostIoSteps<FetchResult>;
}
