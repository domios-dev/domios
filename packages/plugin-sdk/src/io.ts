import type { DirEntry, ExecOpts, ExecPollResult, FetchOpts, FetchResult } from "./host";

type IoAuthoring<T> = T extends {op: "sqliteQuery"} ? Omit<T, "params"> & {params?: unknown[] | null} : T;

export type HostIoRequest = IoAuthoring<import("./wireViews").OptionalWireCases<import("./gen/HostIoRequest").HostIoRequest, "suffix" | "params" | "nullable">> | {op: "awaitJob"; job: string};

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
